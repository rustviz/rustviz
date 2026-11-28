struct Tracked(u8);

impl Drop for Tracked {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

fn run(k: u8) {
    let mut z = Tracked(1);
    let s = Tracked(2);
    match k {
        0 if { z = Tracked(3); false } => { z = Tracked(4); drop(s); }
        _ => {}
    }
    println!("kept {}", z.0);
}

fn main() {
    run(0);
}
