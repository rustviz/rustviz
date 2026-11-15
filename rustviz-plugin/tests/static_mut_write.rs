// Writes to a `static mut` (Rust Book ch20, "Accessing or Modifying a
// Mutable Static Variable"). Each static has a column in the function
// that writes it (#690), where the write is drawn. Moving a non-Copy
// value into the static must show the value as moved, not dropped at
// scope end, and the static's old value as dropped.

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
