/* --- BEGIN Variable Definitions ---
Owner a; Owner b; StaticRef r; StaticRef x; StaticRef y;
--- END Variable Definitions --- */
fn main(){
    let a = 10;
    let b = 6;
    let r: &i32;
    {
        let x: &i32 = &a;
        let y: &i32 = &b;
        r = max(x,y); // !{ Lifetime(<FUNC: max>[x{6:9}*CRPT* bind to variable a *DRPT* dropped as this smaller scope ends][y{7:9}*CRPT* bind to variable b *DRPT* dropped as this smaller scope ends]->[r{4:11}*CRPT* created as reference to i32 type *BODY* can bind to either a or b during runtime *DRPT* dropped as last used by println (also main exits)] )}
    }
    println!("r is {}",r);
}

fn max<'a>(x: &'a i32, y: &'a i32) -> &'a i32{
    if x >= y{
        x
    }
    else{
        y
    }
}