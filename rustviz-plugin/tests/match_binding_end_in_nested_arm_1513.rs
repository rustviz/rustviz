// #1513: a match arm's reference binding last used in one arm of a
// conditional nested in the arm.
fn binding_return(flag: bool) {
    let t = (String::new(), 1);
    match &t {
        (x, 1) => {
            if flag {
                println!("{}", x);
                return;
            }
        }
        _ => {}
    }
}
fn binding_else(flag: bool) {
    let t = (String::new(), 1);
    match &t {
        (x, 1) => {
            if flag {
                println!("{}", x);
            } else {
            }
        }
        _ => {}
    }
}
fn ref_binding_panic(flag: bool) {
    let t = (String::new(), 1);
    match t {
        (ref x, 1) => {
            if flag {
                println!("{}", x);
                panic!();
            }
        }
        _ => {}
    }
}
struct Holder<'a> {
    r: &'a String,
    n: i32,
}
fn field_copy_in_arm(m: &mut String, flag: bool) -> usize {
    let h = Holder { r: &*m, n: 1 };
    let s3 = if flag {
        let r = h.r;
        r.len()
    } else {
        0
    };
    s3
}
fn main() {}
