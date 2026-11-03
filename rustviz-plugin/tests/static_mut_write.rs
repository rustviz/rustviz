// Writes to a `static mut` (Rust Book ch20, "Accessing or Modifying a
// Mutable Static Variable"). The static has no column, like any static,
// so the write itself draws nothing. Moving a non-Copy value into the
// static must still show the value as moved, not dropped at scope end.

static mut COUNTER: u32 = 0;
static mut LOG: Vec<String> = Vec::new();

fn add_to_count(inc: u32) {
    unsafe {
        COUNTER += inc;
    }
}

fn main() {
    add_to_count(3);
    let v = vec![String::from("hi")];
    unsafe {
        LOG = v;
    }
}
