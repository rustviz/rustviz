fn nested(a: [[String; 2]; 2]) {
    let [[s, _], _] = a;
    drop(s);
}

fn reassign(a: [String; 2], next: String) {
    let [mut s, _] = a;
    drop(s);
    s = next;
    drop(s);
}
