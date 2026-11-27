// Borrow checking reads the imagined else entry, including loans and
// partial moves that only occur on some of the incoming paths.
#![allow(irrefutable_let_patterns, unused)]

fn shared_reference_binding(mut n: i32, marker: String) {
    let r = &mut n;
    let q = &r;
    if let binding = marker {
        drop(binding);
    } else {
        // Writing through r here conflicts with q's later use (E0506).
        println!("{}", **q);
    }
    println!("{}", **q);
}

fn conditional_shared(c: bool, a: String, b: String, marker: String) {
    let r = if c { &a } else { &b };
    if let binding = marker {
        drop(binding);
    } else {
        println!("{r}");
    }
    println!("{r}");
}

fn conditional_mutable(c: bool, mut am: String, mut bm: String, marker: String) {
    let r = if c { &mut am } else { &mut bm };
    if let binding = marker {
        drop(binding);
    } else {
        r.push('!');
    }
    r.push('?');
}

fn conditional_enum_move(c: bool, opts: Option<String>, marker: String) {
    if c {
        if let Some(value) = opts {
            drop(value);
        }
    }
    if let binding = marker {
        drop(binding);
    } else {
        println!("the two incoming paths differ");
    }
}

struct G<'a>(&'a mut i32);
impl Drop for G<'_> {
    fn drop(&mut self) {
        *self.0 += 1;
    }
}

fn cleanup(mut at_entry: i32) {
    if let ref g = G(&mut at_entry) {
        let _ = g.0;
    } else {
        // The temporary G's borrow has ended by the brace in edition 2024.
        at_entry += 1;
    }
    println!("{at_entry}");
}

fn pair() -> (String, String) {
    (String::from("taken"), String::from("remaining"))
}

fn temp_drop(marker: String) {
    if let (taken, _) = pair() {
        drop(taken);
    } else {
        drop(marker);
    }
}

fn chain(t: (String,), kept: String) {
    if let (_,) = t && let _ = () {
        println!("matched");
    } else {
        drop(kept);
    }
}

fn before_else_if(m: String, b: String, c: bool) {
    let saved: String;
    if let binding = m {
        saved = binding;
    } else if c {
        drop(b);
    }
    println!("after");
}

fn before_return(input: String) {
    let saved: String;
    if let binding = input {
        saved = binding;
    } else {
        return;
    }
    println!("{saved}");
}

fn partial_at_entry(c: bool, p: (String, String), marker: String) {
    if c {
        drop(p.0);
    }
    if let binding = marker {
        drop(binding);
    } else {
        println!("{}", p.1);
    }
}

struct Split(String, String);

fn whole_move_on_dead_path(c: bool, partial: Split) {
    if let _ = () {
        if c {
            drop(partial.0);
        }
    } else {
        drop(partial);
    }
    // At runtime, partial.1 survives even when partial.0 has moved.
    // Rustc rejects &partial.1 here because the dead else moves partial.
    println!("joined");
}

fn aggregate_initializer(c: bool, a: String) {
    let result = if let binding = a {
        if c { return; }
        Split(binding, String::new())
    } else {
        Split(String::new(), String::new())
    };
    // The binding (including both fields) is in scope after the conditional.
    println!("{}", result.1);
}
