// #1104: a closure handed back as `Box<dyn Fn… + 'a>` keeps the borrows
// it captured. `boxed`'s result doesn't name the closure, but the `F: 'a`
// bound lets it hold the closure under `'a`, so g holds the closure's
// borrows (only `self.n` through `&mut self`). A `Box<dyn Fn>` has drop
// glue: the borrow lasts until g is dropped, at the end of the scope or
// at `drop(g)`. A closure that reads through a shared reference
// (`|x| r.len() …`) reborrows `*r`; m holds that borrow at least until
// its last call.
fn boxed<'a>(f: impl Fn(&i32) -> bool + 'a) -> Box<dyn Fn(&i32) -> bool + 'a> {
    Box::new(f)
}

struct Bag {
    other: Vec<i32>,
    n: usize,
}

impl Bag {
    fn kept(&mut self) -> bool {
        let g = boxed(|x| *x as usize == self.n);
        self.other.push(0);
        g(&1)
    }
}

fn local() -> bool {
    let s = String::from("a");
    let g = boxed(|x| s.len() == *x as usize);
    let ok = g(&1);
    println!("{}", s);
    ok
}

fn dropped() -> bool {
    let mut t = String::from("b");
    let h = boxed(|x| t.len() == *x as usize);
    let found = h(&1);
    drop(h);
    t.push('c');
    found
}

fn plain() -> i32 {
    let u = String::from("c");
    let k: Box<dyn Fn(i32) -> i32 + '_> = Box::new(|x| x + u.len() as i32);
    let a = k(1);
    println!("{}", u);
    a
}

fn through_ref() -> bool {
    let v = String::from("d");
    let r = &v;
    let m = boxed(|x| r.len() == *x as usize);
    let hit = m(&1);
    hit
}

fn main() {
    let mut b = Bag { other: vec![], n: 0 };
    b.kept();
    local();
    dropped();
    plain();
    through_ref();
}
