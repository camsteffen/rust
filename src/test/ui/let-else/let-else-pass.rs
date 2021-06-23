// run-pass

#![feature(let_else)]

fn main() {
    #[allow(dead_code)]
    enum MyEnum {
        A(u32),
        B { f: u32 },
        C,
    }
    let (MyEnum::A(ref x) | MyEnum::B { f: ref x }) = MyEnum::B { f: 1 } else {
        panic!();
    };
    assert_eq!(x, &1);
}
