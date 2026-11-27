struct H<'a>(&'a String);
impl Drop for H<'_> { fn drop(&mut self) {} }

pub fn check(c: bool) {
    let mut b = String::from("b");
    let mut h = H(&b);
    let taken = c && {
        drop(h);
        b.push('x');
        h = H(&b);
        true
    };
    println!("{} {taken}", h.0);
}

pub fn same_holder(c: bool, d: bool) {
    let mut b = String::from("b");
    let mut h = H(&b);
    let taken = c && {
        drop(if d { h } else { h });
        b.push('x');
        h = H(&b);
        true
    };
    println!("{} {taken}", h.0);
}

pub fn different_holders(c: bool, d: bool) {
    let b = String::from("b");
    let e = String::from("e");
    let h = H(&b);
    let k = H(&e);
    let taken = c && {
        drop(if d { h } else { k });
        true
    };
    println!("{taken}");
}
