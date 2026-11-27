enum Choice<'a> { M(&'a mut i32), S(&'a i32) }
enum Pair<'a> { Values(&'a mut i32, i32) }


fn arms(flag: bool) {
    let mut x = 0;
    let p = &mut x;
    let h = if flag {
        Choice::M(p)
    } else {
        Choice::S(&*p)
    };
    std::hint::black_box(h);
}

fn mixed_arms(flag: bool) {
    let a = 1;
    let mut x = 2;
    let mut y = 3;
    let p = &mut x;
    let q = &mut y;
    let bag: Option<(&i32, &mut i32)> = if flag {
        Some((&a, p))
    } else {
        Some((&*p, &mut *q))
    };
    println!("{bag:?}");
}

fn raw() {
    let mut x = 0;
    let source = &mut x;
    let mut y = 2;
    let mut z = 3;
    let raw_parent = &mut y;
    let through = &mut z;
    let raw_pair: Option<(*const i32, *mut i32, &i32)> =
        Some((&raw const *source, &raw mut *raw_parent, through));
    *source = 4;
    *raw_parent = 5;
    std::hint::black_box(raw_pair);
}

fn array() {
    let a = 1;
    let mut b = 2;
    let r = &mut b;
    let refs: [(&i32, &i32); 1] = [(&a, r)];
    println!("{} {:?}", *r, refs);
}

fn alternatives(flag: bool) {
    let mut a = 1;
    let mut b = 2;
    let mut c = 3;
    let parent = &mut c;
    let either: Option<(&i32, &i32)> = Some((if flag { &a } else { &b }, parent));
    println!("{either:?}");
}

fn rows() {
    let mut data = [0];
    let mut x = 1;
    let q = &mut x;
    let mut y = 2;
    let r = &mut y;
    let multiple: Option<(&i32, &i32, &i32, &i32)> = Some({
        *q = 3;
        data[0] = 4;
        (
            &*q,
            &data[0],
            &*q,
            r,
        )
    });
    println!("{multiple:?}");
}

fn reservation() {
    let mut x = 0;
    let r = &mut x;
    let pair = Pair::Values(
        r,
        *r,
    );
    std::hint::black_box(pair);
}

fn coerced(s: &mut String) -> (&str,) { (s,) }
fn maybe(r: &mut i32, gate: Option<()>) -> Option<(&mut i32, ())> {
    Some((r, gate?))
}

fn tuple() {
    let entry = 1;
    let mut x = 2;
    let ptr = &mut x;
    let fields: (&i32, &i32) = (&entry, ptr);
    println!("{} {:?}", *ptr, fields);
    *ptr = 3;
}

fn array_rows() {
    let mut a = 0;
    let mut b = 1;
    let p = &mut a;
    let refs: [(&i32, &i32); 1] = [(
        { b = 2; &b },
        p,
    )];
    println!("{refs:?}");
}

fn main() {}

fn stored(flag: bool) {
    let mut a = 1;
    let b = 2;
    let c = 3;
    let carried: &i32 = &mut a;
    let mut saved = Vec::new();
    saved.push((if flag { carried } else { &b }, &c));
    println!("{b} {c} {saved:?}");
}
