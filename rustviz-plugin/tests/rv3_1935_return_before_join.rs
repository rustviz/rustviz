struct Log(&'static str);

impl Drop for Log {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

struct Pair {
    a: Log,
    b: Log,
}

impl Pair {
    fn into_a(self) -> Log {
        self.a
    }
}

fn run(early: bool) {
    let p = Pair { a: Log("a"), b: Log("b") };
    let out;
    if true {
        if early {
            return;
        }
        out = p.into_a();
    }
    println!("join");
}
