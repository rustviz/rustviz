use std::hint::black_box;
use std::ops::Index;

fn overwrite(i: usize) {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let mut slots = [&a, &b];
    slots[0] = &c;
    let chosen = slots[i];
    assert_eq!(chosen.as_str(), if i == 0 { "c" } else { "b" });
}

fn diagonal(i: usize) {
    let a = String::from("a");
    let b = String::from("b");
    let c = String::from("c");
    let d = String::from("d");
    let matrix = [[&a, &b], [&c, &d]];
    let corner = matrix[i][i];
    assert_eq!(corner.as_str(), if i == 0 { "a" } else { "d" });
}

#[repr(C)]
union Slots<'a> { left: [&'a String; 1], right: [&'a String; 1] }

fn overlapping_fields() {
    let a = String::from("a");
    let b = String::from("b");
    let mut slots = Slots { left: [&a] };
    slots.left = [&a];
    slots.right = [&b];
    let result = unsafe { slots.left[0] };
    assert_eq!(result.as_str(), "b");
}

static ALT: &str = "alternate";
fn redirect(slot: &mut &&str) {
    *slot = &ALT;
    black_box(slot);
}

fn retarget() {
    let entries = vec!["original"];
    let second = "second";
    let mut p = Index::index(&entries, 0);
    let alias = &mut p;
    *alias = &second;
    let direct = *p;
    assert_eq!(direct, "second");

    let mut q = Index::index(&entries, 0);
    redirect(&mut q);
    let called = *q;
    assert_eq!(called, "alternate");
}

fn derefs() {
    let text = "through";
    let slots = [&text, &text];
    let indirect = *slots[0];
    assert_eq!(indirect, "through");
    let backing = ["implicit", "other"];
    let slice = &backing;
    let implicit = slice[0];
    assert_eq!(implicit, "implicit");
    let nested = [vec!["nested"]];
    let i = black_box(0);
    let inner = nested[i][0];
    assert_eq!(inner, "nested");
}

fn reference_slot() {
    let mut text = String::from("slot");
    let p = &mut text;
    let slots = [&p];
    let r = slots[0];
    assert_eq!(r.as_str(), "slot");
}

struct Watch<'a> { r: &'a String }
impl Drop for Watch<'_> {
    fn drop(&mut self) { println!("{}", self.r); }
}

fn keep<'a>(slots: &mut Vec<&'a String>, value: &'a String) { slots.push(value); }

fn destination_drop() {
    let a = String::from("watched");
    let mut slots = Vec::new();
    keep(&mut slots, &a);
    let watch = Watch { r: slots[0] };
    black_box(&watch.r);
    black_box(());
    black_box(&a);
    drop(watch);
    drop(a);
}

struct Shelf<'a, 'b> { items: Vec<&'a String>, guard: Watch<'b> }

fn sibling_drop() {
    let a = String::from("guarded");
    let b = String::from("temporary");
    let shared = &a;
    let mut shelf = Shelf { items: vec![shared], guard: Watch { r: shared } };
    keep(&mut shelf.items, &b);
    let r = shelf.items[0];
    black_box(());
    black_box(&b);
    black_box(r);
    drop(b);
}

fn same_row() {
    let a = String::from("stored");
    let mut slots = Vec::new();
    keep(&mut slots, &a); let r = slots[0];
    black_box(r);
}

fn sibling_arms(c: bool) {
    let a = String::from("arms");
    let mut slots = Vec::new();
    keep(&mut slots, &a);
    if c {
        black_box(());
        black_box(&a);
        let first = slots[0];
        black_box(first);
    } else {
        black_box(());
        black_box(&a);
        let last = slots[0];
        black_box(last);
    }
}

fn main() {
    for i in 0..2 { overwrite(i); diagonal(i); }
    overlapping_fields();
    retarget();
    derefs();
    reference_slot();
    destination_drop();
    sibling_drop();
    same_row();
    sibling_arms(true);
    sibling_arms(false);
}
