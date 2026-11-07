use std::collections::{BTreeMap, HashMap, VecDeque};

fn main() {
    let words = vec![String::from("a"), String::from("b")];
    let queue: VecDeque<String> = VecDeque::new();
    let pair: [String; 2] = [String::from("c"), String::from("d")];
    let slice: Box<[String]> = vec![String::from("e")].into_boxed_slice();
    let maybe: Option<String> = Some(String::from("f"));
    let boxed: Box<String> = Box::new(String::from("g"));
    let both: HashMap<String, String> = HashMap::new();
    let keys: BTreeMap<String, i32> = BTreeMap::new();
    let vals: BTreeMap<i32, String> = BTreeMap::new();
    let rows: Vec<Vec<String>> = Vec::new();
    let nums = vec![1, 2, 3];
    let maybe_num: Option<i32> = Some(1);
    let gone = vec![String::from("h")];
    drop(gone);
    let mut again = vec![String::from("i")];
    again = Vec::new();
    println!("{} {} {} {} {}", words.len(), queue.len(), pair.len(), slice.len(), maybe.is_some());
    println!("{} {} {} {} {}", boxed, both.len(), keys.len(), vals.len(), rows.len());
    println!("{} {} {}", nums.len(), maybe_num.is_some(), again.len());
}
