// A struct bound from an overloaded operator's result (#314): its
// fields own their parts of the new value, and drop them at scope end.
use std::ops::{Add, Neg};

struct Tag { label: String }

impl Add for Tag {
    type Output = Tag;
    fn add(self, other: Tag) -> Tag {
        Tag { label: self.label + &other.label }
    }
}

struct Wave { samples: Vec<i32> }

impl Neg for Wave {
    type Output = Wave;
    fn neg(self) -> Wave {
        Wave { samples: self.samples.iter().map(|x| -x).collect() }
    }
}

fn main() {
    let a = Tag { label: String::from("a") };
    let b = Tag { label: String::from("b") };
    let c = a + b;
    println!("{}", c.label);
    let wave = Wave { samples: vec![1, -2] };
    let flipped = -wave;
    println!("{}", flipped.samples.len());
}
