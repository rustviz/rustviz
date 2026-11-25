// #1486: a holder (or a reference) given the value of a block that
// reborrows a `&mut` the block declares (`v = { let r = &mut a; vec![r] };`).
// v's type is known, so rustc reborrows r there (`&mut *r`). r goes out of
// scope at the block's end, and what v holds from then on is r's loan of a,
// until v's last use (line 18): v gives a back there, and r's own borrow ends
// with its scope. main drew no borrow into v, ended r's borrow after its
// scope and had v give back "*r".
fn main() {
    let mut a = String::from("a");
    let mut z = String::from("z");
    let mut v = vec![&mut z];
    v[0].push('!');
    v = {
        let r = &mut a;
        vec![r]
    };
    v[0].push('?');
    println!("{} {}", a, z);
    typed_let();
    opt();
    reference();
    typed_ref_let();
}

// The same with the type written out on a `let`.
fn typed_let() {
    let mut b = String::from("b");
    let mut v2: Vec<&mut String> = {
        let r2 = &mut b;
        vec![r2]
    };
    v2[0].push('?');
    println!("{}", b);
}

// An `Option<&mut String>` holder.
fn opt() {
    let mut c = String::from("c");
    let mut w = String::from("w");
    let mut y = Some(&mut w);
    y = {
        let r3 = &mut c;
        Some(r3)
    };
    if let Some(q) = y {
        q.push('?');
    }
    println!("{} {}", c, w);
}

// A reference re-pointed at a block's value: x reborrows r4, and holds
// r4's loan of d once r4 is gone.
fn reference() {
    let mut d = String::from("d");
    let mut e = String::from("e");
    let mut x = &mut e;
    x.push('!');
    x = {
        let r4 = &mut d;
        r4
    };
    x.push('?');
    println!("{} {}", d, e);
}

fn typed_ref_let() {
    let mut f = String::from("f");
    let x2: &mut String = {
        let r5 = &mut f;
        r5
    };
    x2.push('?');
    println!("{}", f);
}
