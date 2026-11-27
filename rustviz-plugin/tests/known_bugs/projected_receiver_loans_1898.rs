// Separate the kind of the input loan from the access the stored reference gives.
fn shared_push() {
    let mut a = ([(1, 2)], 3);
    let mut v: Vec<&i32> = Vec::new();
    v.push(&mut a.0[0].0);
    assert_eq!(*v[0], 1);

    let mut b = ([(4, 5)], 6);
    let r: &i32 = &mut b.0[0].0;
    let mut refs = Vec::new();
    refs.push(r);
    assert_eq!(*refs[0], 4);
}

// The iterator supplies no elements.
// Its type still retains the loan.
fn filtered_extend() {
    let a = ([(1, 2)], 3);
    let mut empty: Vec<&i32> = Vec::new();
    empty.extend([&a.0[0].0].into_iter().filter(|_| false));
    assert!(empty.is_empty());
}

// Separate fields of the same array element use the same drawn column.
fn mixed_extend() {
    let mut p = ([(1, 2)], 3);
    let mut out = Vec::new();
    out.extend([(&p.0[0].0, &mut p.0[0].1)]);
    assert_eq!(p.0[0].0, 1);
    *out[0].1 = 8;
    assert_eq!(*out[0].0, 1);
}

fn conditional_push(flag: bool) {
    let a = [1];
    let b = [2];
    let mut out = Vec::new();
    out.push((if flag { &a[0] } else { &b[0] }, 0));
    assert_eq!(*out[0].0, if flag { 1 } else { 2 });
}

fn enum_payload() {
    let a = [Some(1)];
    let mut out = Vec::new();
    if let Some(ref r) = a[0] {
        out.push(r);
        assert_eq!(*out[0], 1);
    }
}

fn main() {
    shared_push();
    filtered_extend();
    mixed_push();
    mixed_extend();
    conditional_push(true);
    conditional_push(false);
    enum_payload();
}

fn mixed_push() {
    let mut pair = ([(1, 2)], 3);
    let mut refs = Vec::new();
    refs.push((&pair.0[0].0, &mut pair.0[0].1));
    assert_eq!(pair.0[0].0, 1);
    *refs[0].1 = 8;
    assert_eq!(*refs[0].0, 1);
}
