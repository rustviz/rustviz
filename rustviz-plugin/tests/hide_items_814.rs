use std::fmt::Write; // rustviz: hide

#[derive(Debug)]
struct Greeter { // rustviz: hide
    s: String,
}

enum Mood { // rustviz: hide
    Happy,
    Sad,
}

impl Greeter { // rustviz: hide
    fn name(&self) -> &String {
        &self.s
    }
}

fn main() {
    let g = Greeter { s: String::from("Ferris") };
    let _m = Mood::Happy;
    let mut out = String::new();
    write!(out, "{}", g.s).unwrap();
    println!("{}", out);
}
