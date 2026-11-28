use std::hint::black_box;
use std::ops::Index;

fn different_slots() {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut slots = [&a, &b];
    slots[0] = &c;
    let different = slots[1];
    let same = slots[0];
    assert_eq!((different.as_str(), same.as_str()), ("b", "c"));
}

fn same_input(i: usize) {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut slots = [&a, &b];
    slots[i] = &c;
    let selected = slots[i];
    assert_eq!(selected.as_str(), "c");
}

fn either_input(i: usize) {
    let a = String::from("a");
    let b = String::from("b");
    let slots = [&a, &b];
    let selected = slots[i];
    black_box(selected);
}

struct Lookup<'a>(&'a str);
static ANSWER: &str = "answer";

impl<'a> Index<usize> for Lookup<'a> {
    type Output = &'a str;
    fn index(&self, _: usize) -> &Self::Output {
        &ANSWER
    }
}

fn opaque() {
    let text = String::from("input");
    let lookup = Lookup(&text);
    let answer = lookup[0];
    assert_eq!(answer, "answer");
}

fn main() {
    different_slots();
    same_input(0);
    same_input(1);
    either_input(0);
    either_input(1);
    opaque();
}
