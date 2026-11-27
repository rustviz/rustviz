fn main() {
    let mut v = vec![1];
    let r = &mut v;
    r.push(
        r.len()
    );
    r.clear();
}

fn nested<'a, 'b>(mut r: &'a mut &'b mut Vec<usize>, other: &'a mut &'b mut Vec<usize>) {
    r.push({
        r = other;
        r.clear();
        0
    });
}

fn shared_binding(r: &mut Vec<usize>) {
    r.push({
        let q = &r;
        q.len()
    });
}

trait Slot {
    fn take(&mut self, len: usize);
}
impl Slot for &mut Vec<usize> {
    fn take(&mut self, _: usize) {}
}

fn binding(mut r: &mut Vec<usize>) {
    r.take(
        r.len()
    );
}
