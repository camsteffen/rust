use rustc_data_structures::fx::FxHashMap;
use rustc_errors::Applicability;
use rustc_hir::{self as hir, ExprKind};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::{declare_lint_pass, declare_tool_lint};
use rustc_span::symbol::Symbol;

use if_chain::if_chain;

use crate::utils::{snippet, span_lint_and_sugg};

declare_clippy_lint! {
    /// **What it does:** Checks for struct constructors where the order of the field init
    /// shorthand in the constructor is inconsistent with the order in the struct definition.
    ///
    /// **Why is this bad?** Since the order of fields in a constructor doesn't affect the
    /// resulted instance as the below example indicates,
    ///
    /// ```rust
    /// #[derive(Debug, PartialEq, Eq)]
    /// struct Foo {
    ///     x: i32,
    ///     y: i32,
    /// }
    /// let x = 1;
    /// let y = 2;
    ///
    /// // This assertion never fails.
    /// assert_eq!(Foo { x, y }, Foo { y, x });
    /// ```
    ///
    /// inconsistent order means nothing and just decreases readability and consistency.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    ///
    /// ```rust
    /// struct Foo {
    ///     x: i32,
    ///     y: i32,
    /// }
    /// let x = 1;
    /// let y = 2;
    /// Foo { y, x };
    /// ```
    ///
    /// Use instead:
    /// ```rust
    /// # struct Foo {
    /// #     x: i32,
    /// #     y: i32,
    /// # }
    /// # let x = 1;
    /// # let y = 2;
    /// Foo { x, y };
    /// ```
    pub INCONSISTENT_STRUCT_CONSTRUCTOR,
    style,
    "the order of the field init shorthand is inconsistent with the order in the struct definition"
}

declare_lint_pass!(InconsistentStructConstructor => [INCONSISTENT_STRUCT_CONSTRUCTOR]);

impl LateLintPass<'_> for InconsistentStructConstructor {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx hir::Expr<'_>) {
        if_chain! {
            if let ExprKind::Struct(qpath, fields, base) = expr.kind;
            if let Some(def_id)  = cx.qpath_res(qpath, expr.hir_id).opt_def_id();
            let ty = cx.tcx.type_of(def_id);
            if let Some(adt_def) = ty.adt_def();
            if adt_def.is_struct();
            if let Some(variant) = adt_def.variants.iter().next();
            if fields.iter().all(|f| f.is_shorthand);
            then {
                let mut def_order_map = FxHashMap::default();
                for (idx, field) in variant.fields.iter().enumerate() {
                    def_order_map.insert(field.ident.name, idx);
                }

                if is_consistent_order(fields, &def_order_map) {
                    return;
                }

                let mut ordered_fields: Vec<_> = fields.iter().map(|f| f.ident.name).collect();
                ordered_fields.sort_unstable_by_key(|id| def_order_map[id]);

                let mut fields_snippet = String::new();
                let (last_ident, idents) = ordered_fields.split_last().unwrap();
                for ident in idents {
                    fields_snippet.push_str(&format!("{}, ", ident));
                }
                fields_snippet.push_str(&last_ident.to_string());

                let base_snippet = if let Some(base) = base {
                        format!(", ..{}", snippet(cx, base.span, ".."))
                    } else {
                        String::new()
                    };

                let sugg = format!("{} {{ {}{} }}",
                    snippet(cx, qpath.span(), ".."),
                    fields_snippet,
                    base_snippet,
                    );

                span_lint_and_sugg(
                    cx,
                    INCONSISTENT_STRUCT_CONSTRUCTOR,
                    expr.span,
                    "inconsistent struct constructor",
                    "try",
                    sugg,
                    Applicability::MachineApplicable,
                )
            }
        }
    }
}

// Check whether the order of the fields in the constructor is consistent with the order in the
// definition.
fn is_consistent_order<'tcx>(fields: &'tcx [hir::Field<'tcx>], def_order_map: &FxHashMap<Symbol, usize>) -> bool {
    let mut cur_idx = usize::MIN;
    for f in fields {
        let next_idx = def_order_map[&f.ident.name];
        if cur_idx > next_idx {
            return false;
        }
        cur_idx = next_idx;
    }

    true
}
