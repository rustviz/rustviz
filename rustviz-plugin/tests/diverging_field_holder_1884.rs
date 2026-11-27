// The loan reaches different field slots on the two paths. Only the selected
// terminator runs. A process exit on the other path never uses the loan.
struct Holder<'a> {
    r: &'a mut String,
}

fn stop(value: &mut String) -> ! {
    panic!("{}", value);
}

fn use_field(c: bool) {
    let mut s = String::new();
    let w = Holder { r: &mut s };
    if c {
        let moved = w;
        stop(moved.r);
    }
    std::process::exit(0);
}

fn main() {}

struct ReadOnDrop<'a>(&'a String);

impl Drop for ReadOnDrop<'_> {
    fn drop(&mut self) { println!("{}", self.0); }
}

fn unwind_reads_field() {
    let text = String::from("kept through cleanup");
    let guard = ReadOnDrop(&text);
    panic!("{}", guard.0);
}

// Borrowck retains a cleanup successor here, but exit does not unwind.
fn exit_with_guard() {
    let text = String::from("never printed");
    let guard = ReadOnDrop(&text);
    std::process::exit(0);
}
