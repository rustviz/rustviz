// #319: moving a struct into a function (or a consuming method) moves
// its nested struct's fields too; they're not dropped at scope end.
struct Inner { v: String }
struct Outer { inner: Inner, id: u32 }

impl Outer {
    fn take_inner(self) -> Inner { self.inner }
}

fn main() {
    let o = Outer { inner: Inner { v: String::from("x") }, id: 1 };
    let i = o.take_inner();
    println!("{}", i.v);
    let q = Outer { inner: Inner { v: String::from("y") }, id: 2 };
    drop(q);
}
