// Chained method calls (#132). `b.get_mut().push(5)` is a
// chain of two method calls — `get_mut` returns `&mut Vec<i32>`
// and `push` is called on that return. Pre-#132 only the inner
// `get_mut` arrow was rendered. Now both arrows surface, both
// attributed (pedagogically) to the base receiver `b`.

struct Buf {
    v: Vec<i32>,
}

impl Buf {
    fn get_mut(&mut self) -> &mut Vec<i32> {
        &mut self.v
    }
}

fn main() {
    let mut b = Buf { v: Vec::new() };
    b.get_mut().push(5);
}
