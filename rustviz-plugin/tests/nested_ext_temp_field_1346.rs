// A let-extended temporary's field borrowed inside a literal (`H { r:
// &make_user().name }`, a tuple, `Some(..)`, an array, a nested struct):
// the borrow is of the field's column, as for `let name =
// &make_user().name;` (#1346).
struct User { name: String, age: u32 }
struct H<'a> { r: &'a String }
struct M<'a> { r: &'a mut u32 }
struct HH<'a> { h: H<'a> }
fn make_user() -> User { User { name: String::from("ann"), age: 3 } }
fn main() {
    let h = H { r: &make_user().name };
    let t = (&make_user().name, 1);
    let o = Some(&make_user().name);
    let a = [&make_user().name];
    let m = M { r: &mut make_user().age };
    let hh = HH { h: H { r: &make_user().name } };
    *m.r += 1;
    println!("{} {} {:?} {} {} {}", h.r, t.0, o, a[0], m.r, hh.h.r);
}
