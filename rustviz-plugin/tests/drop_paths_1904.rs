struct Guard(&'static str);

impl Drop for Guard {
    fn drop(&mut self) {
        println!("{}", self.0);
    }
}

fn fail() -> String {
    panic!("before reassignment");
}

pub fn move_join(consume: bool) {
    let mut s = String::from("old");
    if consume {
        drop(s);
    }
    s = fail();
    drop(s);
}

pub struct Pair {
    taken: Guard,
    kept: Guard,
}

pub fn struct_scope_end(consume: bool, mut p: Pair) {
    if consume {
        drop(p.taken);
        p.taken = Guard("replacement");
    } else {
        drop(p.taken);
    }
}

pub fn struct_early_exit(consume: bool, early: bool, mut p: Pair) {
    if consume {
        drop(p.taken);
    }
    if early {
        return;
    }
    p.taken = Guard("replacement");
}
