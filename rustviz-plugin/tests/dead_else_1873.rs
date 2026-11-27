#![allow(irrefutable_let_patterns, unused_variables, unused_assignments)]

fn deferred(a: String) {
    let value: String;
    if let binding = a {
        value = binding;
    } else {
        println!("dead");
        drop(a);
        println!("still dead");
    }
    println!("join");
}

fn one_line(input: String) {
    let saved: String;
    if let binding = input { saved = binding; } else { drop(input); }
    println!("join");
}
