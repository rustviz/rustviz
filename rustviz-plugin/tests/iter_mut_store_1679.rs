fn set<'a>(slot: &mut &'a String, v: &'a String) {
    *slot = v;
}
fn main() {
    let a = String::from("a");
    let t = String::from("t");
    let mut refs = vec![&a];
    let mut it = refs.iter_mut();
    set(it.next().unwrap(), &t);
    println!("{}", t);
    println!("{}", refs[0]);
}

fn direct() {
    let a = String::from("a");
    let new_value = String::from("new");
    let mut slots = [&a];
    let mut iterator = slots.iter_mut();
    *iterator.next().unwrap() = &new_value;
    println!("{}", new_value);
    println!("{}", slots[0]);
}

fn index_argument() {
    let a = String::from("a");
    let indexed = String::from("new");
    let mut slots = vec![&a];
    set(&mut slots[0], &indexed);
    println!("{indexed}");
    println!("{}", slots[0]);
}

// The setter runs in one arm only: the other arm's read finds the
// lender free (rustc accepts the `clear()` there).
fn setter_in_one_arm(c: bool) {
    let a = String::from("a");
    let mut other_arm = String::from("t");
    let mut arm_refs = vec![&a];
    let mut arm_it = arm_refs.iter_mut();
    if c {
        set(arm_it.next().unwrap(), &other_arm);
    } else {
        println!("{other_arm}");
        other_arm.clear();
    }
    println!("{}", arm_refs[0]);
}

fn keep<'a>(v: &mut Vec<&'a String>, s: &'a String) {
    v.push(s);
}

fn keep_in_one_arm(c: bool) {
    let a = String::from("a");
    let mut kept_arm = String::from("t");
    let mut kept = vec![&a];
    if c {
        keep(&mut kept, &kept_arm);
    } else {
        println!("{kept_arm}");
        kept_arm.clear();
    }
    println!("{}", kept[0]);
}

// In a loop, a later pass through the other arm still finds the loan
// of an earlier pass: the read there keeps loop_t shared.
fn setter_in_loop_arm(n: usize) {
    let a = String::from("a");
    let loop_t = String::from("t");
    let mut loop_refs = vec![&a];
    for i in 0..n {
        if i == 0 {
            keep(&mut loop_refs, &loop_t);
        } else {
            println!("{loop_t}");
        }
    }
    println!("{}", loop_refs.len());
}

// A reference variable borrows row_t on the setter's row and outlives
// row_slots: after row_slots is gone only r holds row_t.
fn two_loans_one_row() {
    let a = String::from("a");
    let row_t = String::from("t");
    let r;
    {
        let mut row_slots = vec![&a];
        r = &row_t; set(&mut row_slots[0], &row_t);
        println!("{}", row_slots[0]);
    }
    println!("{r} {row_t}");
}
