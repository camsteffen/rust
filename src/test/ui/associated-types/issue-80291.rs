// run-pass

trait ATrait<'a> {
    type AType;
}

struct C {}

impl<'a> ATrait<'a> for C {
    type AType = u64;
}

fn calc<'b, O, F>(f: F)
where
    O: ATrait<'b>,
    // test that `ATrait::'a` is correctly inferred (previously ICEd at codegen)
    F: Fn(<O as ATrait>::AType),
{
    f(None.unwrap());
}

fn main() {
    calc::<C, _>(|_| {});
}
