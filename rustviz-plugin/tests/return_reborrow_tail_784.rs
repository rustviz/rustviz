// #784: a returned reborrow through a reference parameter (`&self.s`)
// hands `self`'s borrow to the caller, like returning `self` itself
// does: the caller's value stays borrowed while the result is used, so
// "self's borrow ends" would be false. On the other path of a tail
// `if` / `match`, a parameter whose lifetime the signature ties to the
// result gets the dot from #775; one with a lifetime of its own
// (`other: &str` in `untied`) still ends its borrow there.
struct Holder {
    s: String,
    v: Vec<i32>,
}

impl Holder {
    fn name(&self) -> &String {
        &self.s
    }

    fn first(&self) -> &i32 {
        &self.v[0]
    }

    fn name_mut(&mut self) -> &mut String {
        &mut self.s
    }

    fn both<'a>(&'a self, c: bool, other: &'a str) -> &'a str {
        if c { &self.s } else { other }
    }

    fn by_match<'a>(&'a self, n: i32, other: &'a str) -> &'a str {
        match n {
            0 => &self.s,
            _ => other,
        }
    }

    fn early(&self, c: bool) -> &str {
        if c {
            return &self.s;
        }
        "none"
    }

    fn pick<'a>(&'a mut self, c: bool, other: &'a mut String) -> &'a mut String {
        if c { &mut self.s } else { other }
    }

    fn untied<'a>(&'a self, c: bool, other: &str) -> &'a str {
        if c { &self.s } else { "none" }
    }
}

fn main() {
    let mut h = Holder { s: String::from("h"), v: vec![1] };
    let mut o = String::from("o");
    println!("{} {} {}", h.name(), h.first(), h.early(true));
    println!("{} {}", h.both(true, &o), h.by_match(1, &o));
    h.name_mut().push('x');
    h.pick(true, &mut o).push('y');
    println!("{}", h.untied(true, &o));
}
