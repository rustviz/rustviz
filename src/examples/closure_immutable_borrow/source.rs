fn main(){
    let nums = vec![1,2,3];
    
    let nth = |x| nums[x];
    
    println!("{}",nth(1));
    
    println!("{}",nums[1]);
}
