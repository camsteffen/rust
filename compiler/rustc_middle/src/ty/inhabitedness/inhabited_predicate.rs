use rustc_macros::StableHash;
use rustc_span::def_id::{LocalModId, ModId};
use rustc_type_ir::TypeFoldable;
use smallvec::SmallVec;
use tracing::instrument;

use crate::ty::{self, EarlyBinder, OpaqueTypeKey, Ty, TyCtxt, TypingEnv, Unnormalized};

/// Represents whether some type is inhabited in a given context.
/// Examples of uninhabited types are `!`, `enum Void {}`, or a struct
/// containing either of those types.
/// A type's inhabitedness may depend on the `ParamEnv` as well as what types
/// are visible in the current module.
#[derive(Clone, Copy, Debug, PartialEq, StableHash)]
pub enum InhabitedPredicate<'tcx> {
    /// Inhabited
    True,
    /// Uninhabited
    False,
    /// Uninhabited when a const value is non-zero. This occurs when there is an
    /// array of uninhabited items, but the array is inhabited if it is empty.
    ConstIsZero(ty::Const<'tcx>),
    /// Uninhabited if within a certain module. This occurs when an uninhabited
    /// type has restricted visibility.
    NotInModule(ModId),
    /// Inhabited if some generic type is inhabited.
    GenericType(Ty<'tcx>),
    /// Nested types are lazily instantiated with the generic args
    Instantiate(&'tcx InhabitedPredicate<'tcx>, ty::GenericArgsRef<'tcx>),
    /// Inhabited if either we don't know the hidden type or we know it and it is inhabited.
    OpaqueType(OpaqueTypeKey<'tcx>),
    /// A AND B
    And(&'tcx [InhabitedPredicate<'tcx>; 2]),
    /// A OR B
    Or(&'tcx [InhabitedPredicate<'tcx>; 2]),
}

impl<'tcx> InhabitedPredicate<'tcx> {
    /// Returns true if the corresponding type is inhabited in the given `ParamEnv` and module.
    pub fn apply(
        self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        module_def_id: LocalModId,
    ) -> bool {
        self.apply_revealing_opaque(tcx, typing_env, module_def_id, &|_| None)
    }

    /// Returns true if the corresponding type is inhabited in the given `ParamEnv` and module,
    /// revealing opaques when possible.
    pub fn apply_revealing_opaque(
        self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        module_def_id: LocalModId,
        reveal_opaque: &impl Fn(OpaqueTypeKey<'tcx>) -> Option<Ty<'tcx>>,
    ) -> bool {
        let Ok(result) = self.apply_inner::<!>(
            tcx,
            typing_env,
            &|id| Ok(tcx.is_descendant_of(module_def_id, id)),
            reveal_opaque,
        );
        result
    }

    /// Same as `apply`, but returns `None` if self contains a module predicate
    pub fn apply_any_module(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> Option<bool> {
        self.apply_inner(tcx, typing_env, &|_| Err(()), &|_| None).ok()
    }

    /// Same as `apply`, but `NotInModule(_)` predicates yield `false`. That is,
    /// privately uninhabited types are considered always uninhabited.
    pub fn apply_ignore_module(self, tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> bool {
        let Ok(result) = self.apply_inner::<!>(tcx, typing_env, &|_| Ok(true), &|_| None);
        result
    }

    #[instrument(level = "debug", skip(tcx, typing_env, in_module, reveal_opaque), ret)]
    fn apply_inner<E: std::fmt::Debug>(
        self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        in_module: &impl Fn(ModId) -> Result<bool, E>,
        reveal_opaque: &impl Fn(OpaqueTypeKey<'tcx>) -> Option<Ty<'tcx>>,
    ) -> Result<bool, E> {
        InhabitedPredicateEval {
            tcx,
            typing_env,
            args: None,
            in_module,
            reveal_opaque,
            eval_stack: Default::default(),
        }
        .eval_pred(self)
    }

    pub fn and(self, tcx: TyCtxt<'tcx>, other: Self) -> Self {
        self.reduce_and(tcx, other).unwrap_or_else(|| Self::And(tcx.arena.alloc([self, other])))
    }

    pub fn or(self, tcx: TyCtxt<'tcx>, other: Self) -> Self {
        self.reduce_or(tcx, other).unwrap_or_else(|| Self::Or(tcx.arena.alloc([self, other])))
    }

    pub fn all(tcx: TyCtxt<'tcx>, iter: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::True;
        for pred in iter {
            if pred == Self::False {
                return Self::False;
            }
            result = result.and(tcx, pred);
        }
        result
    }

    pub fn any(tcx: TyCtxt<'tcx>, iter: impl IntoIterator<Item = Self>) -> Self {
        let mut result = Self::False;
        for pred in iter {
            if pred == Self::True {
                return Self::True;
            }
            result = result.or(tcx, pred);
        }
        result
    }

    fn reduce_and(self, tcx: TyCtxt<'tcx>, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::True, a) | (a, Self::True) => Some(a),
            (Self::False, _) | (_, Self::False) => Some(Self::False),
            (Self::ConstIsZero(a), Self::ConstIsZero(b)) if a == b => Some(Self::ConstIsZero(a)),
            (Self::NotInModule(a), Self::NotInModule(b)) if a == b => Some(Self::NotInModule(a)),
            (Self::NotInModule(a), Self::NotInModule(b)) if tcx.is_descendant_of(a, b) => {
                Some(Self::NotInModule(b))
            }
            (Self::NotInModule(a), Self::NotInModule(b)) if tcx.is_descendant_of(b, a) => {
                Some(Self::NotInModule(a))
            }
            (Self::GenericType(a), Self::GenericType(b)) if a == b => Some(Self::GenericType(a)),
            (Self::Instantiate(&a, a_args), Self::Instantiate(&b, b_args))
                if a_args == b_args
                    && let Some(c) = a.reduce_and(tcx, b) =>
            {
                Some(Self::Instantiate(tcx.arena.alloc(c), a_args))
            }
            (Self::And(&[a, b]), c) | (c, Self::And(&[a, b])) => {
                if let Some(ac) = a.reduce_and(tcx, c) {
                    Some(ac.and(tcx, b))
                } else if let Some(bc) = b.reduce_and(tcx, c) {
                    Some(Self::And(tcx.arena.alloc([a, bc])))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn reduce_or(self, tcx: TyCtxt<'tcx>, other: Self) -> Option<Self> {
        match (self, other) {
            (Self::True, _) | (_, Self::True) => Some(Self::True),
            (Self::False, a) | (a, Self::False) => Some(a),
            (Self::ConstIsZero(a), Self::ConstIsZero(b)) if a == b => Some(Self::ConstIsZero(a)),
            (Self::NotInModule(a), Self::NotInModule(b)) if a == b => Some(Self::NotInModule(a)),
            (Self::NotInModule(a), Self::NotInModule(b)) if tcx.is_descendant_of(a, b) => {
                Some(Self::NotInModule(a))
            }
            (Self::NotInModule(a), Self::NotInModule(b)) if tcx.is_descendant_of(b, a) => {
                Some(Self::NotInModule(b))
            }
            (Self::GenericType(a), Self::GenericType(b)) if a == b => Some(Self::GenericType(a)),
            (Self::Instantiate(&a, a_args), Self::Instantiate(&b, b_args))
                if a_args == b_args
                    && let Some(c) = a.reduce_or(tcx, b) =>
            {
                Some(Self::Instantiate(tcx.arena.alloc(c), a_args))
            }
            (Self::Or(&[a, b]), c) | (c, Self::Or(&[a, b])) => {
                if let Some(ac) = a.reduce_or(tcx, c) {
                    Some(ac.or(tcx, b))
                } else if let Some(bc) = b.reduce_or(tcx, c) {
                    Some(Self::Or(tcx.arena.alloc([a, bc])))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub fn instantiate(self, tcx: TyCtxt<'tcx>, args: ty::GenericArgsRef<'tcx>) -> Self {
        match self {
            Self::True | Self::False => self,
            _ => Self::Instantiate(tcx.arena.alloc(self), args),
        }
    }
}

struct InhabitedPredicateEval<'tcx, InModule, RevealOpaque> {
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    args: Option<ty::GenericArgsRef<'tcx>>,
    eval_stack: SmallVec<[Ty<'tcx>; 1]>,
    in_module: InModule,
    reveal_opaque: RevealOpaque,
}

impl<'tcx, InModule, RevealOpaque, E> InhabitedPredicateEval<'tcx, InModule, RevealOpaque>
where
    InModule: Fn(ModId) -> Result<bool, E>,
    RevealOpaque: Fn(OpaqueTypeKey<'tcx>) -> Option<Ty<'tcx>>,
{
    fn eval_pred(&mut self, pred: InhabitedPredicate<'tcx>) -> Result<bool, E> {
        let tcx = self.tcx;
        match pred {
            InhabitedPredicate::False => Ok(false),
            InhabitedPredicate::True => Ok(true),
            InhabitedPredicate::ConstIsZero(const_) => match const_.try_to_target_usize(tcx) {
                None | Some(0) => Ok(true),
                Some(1..) => Ok(false),
            },
            InhabitedPredicate::NotInModule(id) => (self.in_module)(id).map(|in_mod| !in_mod),
            // `t` may be a projection, for which `inhabited_predicate` returns a `GenericType`. As
            // we have a param_env available, we can do better.
            InhabitedPredicate::GenericType(t) => {
                // A type which is cyclic when monomorphized can happen here since the
                // layout error would only trigger later. See e.g. `tests/ui/sized/recursive-type-2.rs`.
                self.eval_ty(t)
            }
            InhabitedPredicate::Instantiate(&pred, args) => {
                let next_args = self.instantiate(args).unwrap_or(args);
                let args_prev = std::mem::replace(&mut self.args, Some(next_args));
                let out = self.eval_pred(pred);
                self.args = args_prev;
                out
            }
            InhabitedPredicate::OpaqueType(key) => {
                assert!(
                    self.args.is_none(),
                    "InhabitedPredicate::OpaqueType should not be instantiated"
                );
                match (self.reveal_opaque)(key) {
                    // Unknown opaque is assumed inhabited.
                    None => Ok(true),
                    // Known opaque type is inspected recursively.
                    Some(t) => {
                        // A cyclic opaque type can happen in corner cases that would only error later.
                        // See e.g. `tests/ui/type-alias-impl-trait/recursive-tait-conflicting-defn.rs`.
                        self.eval_ty(t)
                    }
                }
            }
            InhabitedPredicate::And([a, b]) => try_and(a, b, |&pred| self.eval_pred(pred)),
            InhabitedPredicate::Or([a, b]) => try_or(a, b, |&pred| self.eval_pred(pred)),
        }
    }

    fn eval_ty(&mut self, t: Ty<'tcx>) -> Result<bool, E> {
        if self.eval_stack.contains(&t) {
            return Ok(true); // Recover; this will error later.
        }
        self.eval_stack.push(t);
        let ret = match self.instantiate(t) {
            // We don't have more information than we started with, so consider inhabited.
            None => Ok(true),
            Some(t) => {
                let pred = t.inhabited_predicate(self.tcx);
                let args_prev = self.args.take();
                let ret = self.eval_pred(pred);
                self.args = args_prev;
                ret
            }
        };
        self.eval_stack.pop();
        ret
    }

    fn instantiate<T: TypeFoldable<TyCtxt<'tcx>> + Copy>(&self, t: T) -> Option<T> {
        let Some(args) = self.args else {
            return None;
        };
        let mut t = EarlyBinder::bind(self.tcx, t).instantiate(self.tcx, args).skip_norm_wip();
        if let Ok(norm) =
            self.tcx.try_normalize_erasing_regions(self.typing_env, Unnormalized::new_wip(t))
        {
            t = norm;
        }
        Some(t)
    }
}

// this is basically like `f(a)? && f(b)?` but different in the case of
// `Ok(false) && Err(_) -> Ok(false)`
fn try_and<T, E>(a: T, b: T, mut f: impl FnMut(T) -> Result<bool, E>) -> Result<bool, E> {
    let a = f(a);
    if matches!(a, Ok(false)) {
        return Ok(false);
    }
    match (a, f(b)) {
        (_, Ok(false)) | (Ok(false), _) => Ok(false),
        (Ok(true), Ok(true)) => Ok(true),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

fn try_or<T, E>(a: T, b: T, mut f: impl FnMut(T) -> Result<bool, E>) -> Result<bool, E> {
    let a = f(a);
    if matches!(a, Ok(true)) {
        return Ok(true);
    }
    match (a, f(b)) {
        (_, Ok(true)) | (Ok(true), _) => Ok(true),
        (Ok(false), Ok(false)) => Ok(false),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}
