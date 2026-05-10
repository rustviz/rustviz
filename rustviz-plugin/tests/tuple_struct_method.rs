// Inherent method on a tuple struct — numeric field idents
// (`self.0`, `self.1`) aren't entered into id_map, so the
// source-annotation step used to panic. Pins the no-crash
// baseline for tuple-struct field access.

struct Pair(i32, i32);

impl Pair {
    fn sum(&self) -> i32 {
        self.0 + self.1
    }
}

fn main() {
    let p = Pair(3, 4);
    let _s = p.sum();
}
