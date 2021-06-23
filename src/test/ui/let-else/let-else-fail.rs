#![feature(let_else)]

fn main() {
    let Some(x) = Some(1) else { //~ ERROR mismatched types
        Some(2);
    };
    let Some(x) = Some(1) else {
        panic!("should match");
    };
}
