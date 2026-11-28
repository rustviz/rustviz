enum Bag<'a> {
    Occupied { item: &'a String, flagged: bool },
}

impl<'a> Bag<'a> {
    fn inspect(&mut self, _: &'a String) {
        let Bag::Occupied { ref mut flagged, .. } = *self;
        *flagged = true;
    }
}

fn inspect_receiver() {
    let kept = String::from("kept");
    let inspected = String::from("inspected");
    let mut bag = Bag::Occupied { item: &kept, flagged: false };
    bag.inspect(&inspected);
    let Bag::Occupied { item, flagged } = bag;
    assert!(std::ptr::eq(item, &kept));
    assert!(flagged);
}

enum Maybe<'a> {
    Data(&'a String),
    Empty,
}

fn maybe<'a>(a: &'a String, b: &'a String, mode: u8) -> Maybe<'a> {
    match mode {
        0 => Maybe::Data(a),
        1 => Maybe::Data(b),
        _ => Maybe::Empty,
    }
}

enum Always<'a> {
    Left(&'a String),
    Right(&'a String),
}

fn always<'a>(a: &'a String, b: &'a String, left: bool) -> Always<'a> {
    if left { Always::Left(a) } else { Always::Right(b) }
}

fn pick<'a>(a: &'a String, _: &'a String, some: bool) -> Option<&'a String> {
    if some { Some(a) } else { None }
}

fn returned_variants() {
    let a = String::from("a");
    let b = String::from("b");
    let perhaps = maybe(&a, &b, 2);
    let definitely = always(&a, &b, false);
    assert!(matches!(perhaps, Maybe::Empty));
    assert!(matches!(definitely, Always::Right(r) if std::ptr::eq(r, &b)));
    if let Some(x) = pick(&a, &b, true) {
        assert!(std::ptr::eq(x, &a));
    }
}

fn maybe_replace<'a>(slot: (&mut &'a String,), replacement: &'a String, set: bool) {
    if set {
        *slot.0 = replacement;
    }
}

fn aggregate_write(set: bool) {
    let alpha = String::from("alpha");
    let beta = String::from("beta");
    let gamma = String::from("gamma");
    let mut left = &alpha;
    let mut right = &beta;
    maybe_replace((&mut left,), &gamma, set);
    std::mem::swap(&mut left, &mut right);
    assert!(std::ptr::eq(left, &beta));
    assert!(std::ptr::eq(right, if set { &gamma } else { &alpha }));
}

struct ReplaceOnDrop<'a, 'b>(&'b mut &'a String, &'a String);

impl Drop for ReplaceOnDrop<'_, '_> {
    fn drop(&mut self) {
        *self.0 = self.1;
    }
}

fn destructor_write() {
    let alpha = String::from("alpha");
    let beta = String::from("beta");
    let gamma = String::from("gamma");
    let mut left = &alpha;
    let mut right = &beta;
    {
        let _ = ReplaceOnDrop(&mut left, &gamma);
    }
    std::mem::swap(&mut left, &mut right);
    assert!(std::ptr::eq(left, &beta));
    assert!(std::ptr::eq(right, &gamma));
}

fn index_write(i: usize, j: usize) {
    let alpha = String::from("alpha");
    let beta = String::from("beta");
    let gamma = String::from("gamma");
    let mut slots = [&alpha, &alpha];
    slots[i] = &gamma;
    let mut left = slots[j];
    let mut right = &beta;
    std::mem::swap(&mut left, &mut right);
    assert!(std::ptr::eq(left, &beta));
    assert!(std::ptr::eq(right, if i == j { &gamma } else { &alpha }));
}

fn payload_swap() {
    let mut a = String::from("a");
    let mut b = String::from("b");
    let mut e = Some(&mut a);
    let mut other = &mut b;
    if let Some(ref mut payload) = e {
        std::mem::swap(&mut *payload, &mut other);
        payload.push_str("!");
        std::hint::black_box(&**payload);
    }
    assert_eq!(other.as_str(), "a");
    std::hint::black_box(&e);
}

fn first<'a>(a: &'a String, _: &'a String) -> &'a String {
    a
}

fn tag() -> u8 {
    42
}

fn sibling_call() {
    let a = String::from("a");
    let b = String::from("b");
    let pair = (first(&a, &b), tag());
    assert!(std::ptr::eq(pair.0, &a));
    assert_eq!(pair.1, 42);
}

fn keep<F>(f: F) -> F {
    f
}

fn unique_capture() {
    let mut value = false;
    let x = &mut value;
    let mut g = keep(|| {
        let _shared = &x;
        *x = true;
    });
    g();
    assert!(value);
}

fn main() {
    inspect_receiver();
    returned_variants();
    aggregate_write(true);
    aggregate_write(false);
    destructor_write();
    index_write(0, 1);
    index_write(0, 0);
    payload_swap();
    sibling_call();
    unique_capture();
}
