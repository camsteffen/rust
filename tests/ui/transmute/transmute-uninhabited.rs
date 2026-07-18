#![feature(never_type)]

use std::mem::transmute;

enum Never {}

unsafe fn uninhabited_to_uninhabited(n: Never) {
    let _ = transmute::<Never, !>(n);
}

unsafe fn unit_to_uninhabited() {
    let _ = transmute::<Option<!>, !>(None);
    //~^ ERROR cannot transmute between types of different sizes, or dependently-sized types
}

unsafe fn uninhabited_to_unit(n: !) {
    let _ = transmute::<!, Option<!>>(n);
    //~^ ERROR cannot transmute between types of different sizes, or dependently-sized types
}

fn main() {}
