// `&mut self` method that assigns to a field — the plugin
// didn't register `self.field` as a RAP for ref-to-struct
// parameters, so the LHS lookup used to panic. This fixture
// pins the no-crash baseline until per-field RAP registration
// for `&Struct` / `&mut Struct` parameters lands.

struct Counter {
    c: i32,
}

impl Counter {
    fn bump(&mut self) {
        self.c = self.c + 1;
    }
}

fn main() {
    let mut c = Counter { c: 0 };
    c.bump();
}
