// Method chains (#132). Only the first call in a chain acts on the
// named variable; every later call acts on a temporary (the previous
// call's result), which has no column:
//   - `b.get_mut().push(5)`: `get_mut` mutably borrows `b`; `push`
//     is called on the `&mut Vec<i32>` that `get_mut` returned.
//   - `v.iter().map(..).collect()`: `iter` borrows `v`; `map` takes
//     the iterator by value and `collect` takes the `Map` adapter.
//   - `s.clone().into_bytes()`: `clone` only reads `s`; it is the
//     clone that `into_bytes` consumes, so `s` still owns its String.

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
    let v = vec![1, 2, 3];
    let doubled: Vec<i32> = v.iter().map(|x| x * 2).collect();
    let s = String::from("hi");
    let bytes = s.clone().into_bytes();
    println!("{:?} {:?} {:?} {}", doubled, v, bytes, s);
}
