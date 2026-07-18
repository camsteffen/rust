#![feature(never_type)]

enum Void {}
extern "C" {
    static VOID: Void; //~ ERROR static of uninhabited type
    //~| WARN: previously accepted
    static NEVER: !; //~ ERROR static of uninhabited type
    //~| WARN: previously accepted
}

static VOID2: Void = unsafe { std::mem::transmute(()) }; //~ ERROR static of uninhabited type
//~| WARN: previously accepted
//~| ERROR cannot transmute between types of different sizes
static NEVER2: Void = unsafe { std::mem::transmute(()) }; //~ ERROR static of uninhabited type
//~| WARN: previously accepted
//~| ERROR cannot transmute between types of different sizes

fn main() {}
