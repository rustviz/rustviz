// #866: a value a constructor builds, and a Copy
// payload a pattern binds. `y = Fruit::Apple(z)` gives y
// a new Fruit built from z's u32, not a copy of z.
// `opt = Some(z + 2)` builds a new Option; `Some` is not
// an actor. `Some(x) = opt` / `Fruit::Apple(n) =>` copy
// the value inside opt / f, not opt / f itself.
enum Fruit {
    Apple(u32),
    Banana,
}

enum Pair {
    Two(u32, u32),
}

fn read(_n: u32) {}

fn main() {
    let z: u32 = 4;
    let f = Fruit::Apple(z);
    let mut y = Fruit::Banana;
    y = Fruit::Apple(z);
    let o = Some(z + 1);
    let mut opt: Option<u32> = None;
    opt = Some(z + 2);
    if let Some(x) = opt {
        read(x);
    }
    let Some(w) = o else { return };
    read(w);
    match f {
        Fruit::Apple(n) => read(n),
        Fruit::Banana => {}
    }
    let p = Pair::Two(1, 2);
    match p {
        Pair::Two(a, b) => read(a + b),
    }
    if let Fruit::Apple(k) = y {
        read(k);
    }
}
