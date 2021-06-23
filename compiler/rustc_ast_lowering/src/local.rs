use super::{ImplTraitContext, ImplTraitPosition, LoweringContext};

use rustc_ast::ptr::P as AstP;
use rustc_ast::*;
use rustc_data_structures::fx::{FxHashSet, FxIndexMap};
use rustc_data_structures::thin_vec::ThinVec;
use rustc_hir as hir;
use rustc_session::parse::feature_err;
use rustc_span::symbol::Ident;
use rustc_span::{sym, DUMMY_SP};

impl<'hir> LoweringContext<'_, 'hir> {
    pub(super) fn lower_local_ty(&mut self, ty: &Option<AstP<Ty>>) -> Option<&'hir hir::Ty<'hir>> {
        ty.as_ref().map(|t| {
            let mut capturable_lifetimes;
            let itcxt = if self.sess.features_untracked().impl_trait_in_bindings {
                capturable_lifetimes = FxHashSet::default();
                ImplTraitContext::OtherOpaqueTy {
                    capturable_lifetimes: &mut capturable_lifetimes,
                    origin: hir::OpaqueTyOrigin::Binding,
                }
            } else {
                ImplTraitContext::Disallowed(ImplTraitPosition::Binding)
            };
            self.lower_ty(t, itcxt)
        })
    }

    pub(super) fn lower_local_let_else(
        &mut self,
        l: &Local,
        scrutinee: &Expr,
        els: &Block,
    ) -> hir::Local<'hir> {
        let ty = self.lower_local_ty(&l.ty);
        let mut pats = Vec::new();
        let then_arm = {
            let (pat, else_bindings) = {
                self.else_bindings = Some(FxIndexMap::default());
                let pat = self.lower_pat(&l.pat);
                (pat, self.else_bindings.take().unwrap())
            };
            let mut exprs = Vec::new();
            for (outer_id, (inner_id, ident)) in else_bindings {
                let outer_pat = &*self.arena.alloc(hir::Pat {
                    hir_id: outer_id,
                    kind: hir::PatKind::Binding(
                        hir::BindingAnnotation::Unannotated,
                        outer_id,
                        ident,
                        None,
                    ),
                    span: ident.span,
                    default_binding_modes: true,
                });
                pats.push(outer_pat);
                exprs.push(self.expr_ident_mut(
                    DUMMY_SP,
                    Ident::with_dummy_span(ident.name),
                    inner_id,
                ));
            }
            let then_expr = {
                let kind = hir::ExprKind::Tup(self.arena.alloc_from_iter(exprs));
                self.expr(l.pat.span, kind, ThinVec::new())
            };
            self.arm(pat, self.arena.alloc(then_expr))
        };
        let else_arm = {
            let else_pat = self.pat_wild(els.span);
            let else_expr = {
                let local = hir::Local {
                    hir_id: self.next_id(),
                    init: Some(self.arena.alloc(self.lower_block_expr(els))),
                    pat: self.pat_wild(DUMMY_SP),
                    source: hir::LocalSource::Normal,
                    span: DUMMY_SP,
                    ty: Some(self.arena.alloc(self.ty(DUMMY_SP, hir::TyKind::Never))),
                };
                let stmt = self.stmt(DUMMY_SP, hir::StmtKind::Local(self.arena.alloc(local)));
                let block = self.block_all(DUMMY_SP, arena_vec![self; stmt], None);
                &*self.arena.alloc(self.expr_block(block, ThinVec::new()))
            };
            self.arm(else_pat, else_expr)
        };
        let init = {
            let desugar = hir::MatchSource::Normal;
            let node = hir::ExprKind::Match(
                self.lower_expr(scrutinee),
                arena_vec![self; then_arm, else_arm],
                desugar,
            );
            Some(&*self.arena.alloc(self.expr(scrutinee.span, node, ThinVec::new())))
        };
        let pat = {
            let node = hir::PatKind::Tuple(self.arena.alloc_slice(&pats), None);
            self.pat(l.pat.span, node)
        };
        let hir_id = self.lower_node_id(l.id);
        self.lower_attrs(hir_id, &l.attrs);
        let source = hir::LocalSource::Normal;
        if !self.sess.features_untracked().let_else {
            feature_err(
                &self.sess.parse_sess,
                sym::let_else,
                l.span,
                "let else statements are unstable",
            )
            .emit();
        }
        hir::Local { hir_id, ty, pat, init, span: l.span, source }
    }
}
