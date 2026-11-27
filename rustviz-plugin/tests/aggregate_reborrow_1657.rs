fn main() {
    let a = 1;
    let mut b = 2;
    let r = &mut b;
    let h: Option<(&i32, &i32)> = Some((&a, r));
    let n = *r + h.unwrap().1;
    *r = 3;
}

struct Pair<'a> { first: &'a i32, second: &'a i32 }

fn struct_field() {
    let a = 1;
    let mut b = 2;
    let s = &mut b;
    let pair = Some(Pair { first: &a, second: s });
    let n = *s + pair.unwrap().second;
    *s = 3;
}

fn mutable_element() {
    let a = 1;
    let mut b = 2;
    let m = &mut b;
    let both: Option<(&i32, &mut i32)> = Some((&a, m));
    *both.unwrap().1 += 1;
    *m = 3;
}

fn inferred_element() {
    let mut b = 2;
    let original = &mut b;
    let moved = Some((original,));
    *moved.unwrap().0 += 1;
}

fn raw_element() {
    let mut b = 2;
    let source = &mut b;
    let ptr: Option<(*const i32,)> = Some((source,));
    *source = 3;
    let address = ptr.unwrap().0;
}

fn on_one_arm(flag: bool) {
    let first = 1;
    let mut second = 2;
    let p = &mut second;
    let bag: Option<(&i32, &mut i32)> = if flag { Some((&first, p)) } else { None };
    println!("{bag:?}");
    *p = 3;
}

fn multiline(parent: &mut i32) {
    let sibling = 1;
    let later: Option<(&i32, &i32)> = Some({
        (
            &sibling,
            {
                *parent += 1;
                parent
            },
        )
    });
    let n = *later.unwrap().1;
    *parent = n;
}
