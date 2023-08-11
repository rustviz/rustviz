fn main() {
    let x = 5;
    let y = x;
}

struct foo<'i>{
    i : &'i i32
}

impl<'i> foo<'i> {
    pub fn sms(& 'i self, other: &'i i32) -> &'i i32{
        
    }
}

fn max<'i,T>(xb: &'i mut T,
                 yb: &'i Vec<u32>,
                ) -> &'i Vec<String> {
    println!("{}",xb);
    xb
}

fn greet<'i>(words: &'i String){
    println("{}", words);
}
//zb: &'i mut ( Vec<i32>, BTreeMap<String, (u32, Vec<String>)> ),