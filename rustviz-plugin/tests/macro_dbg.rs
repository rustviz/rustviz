struct Rect {
    w: u32,
}

impl std::fmt::Debug for Rect {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Rect({})", self.w)
    }
}

fn main() {
    let a = Rect { w: 1 };
    dbg!(&a);
    let b = dbg!(a);
    let r = dbg!(&b);
    println!("{:?}", r);
    let c = Rect { w: 2 };
    dbg!(c);
    let x = 3;
    dbg!(x);
}
