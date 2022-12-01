use std::thread; 
fn <tspan class="fn" data-hash="0" hash="4">parent</tspan>() { 
    let mut <tspan data-hash="6">v</tspan> = <tspan class="fn" data-hash="0" hash="5">vec!</tspan>[1 , 2 , 3]; 
    let <tspan data-hash="7">handle</tspan> = <tspan class="fn" data-hash="0" hash="3">thread::spawn</tspan>( move || { 
        v.push(4); // OK , the thread now owns v due to move keyword 
    }); 
    <tspan data-hash="7">handle</tspan>.<tspan class="fn" data-hash="0" hash="2">join()</tspan>.<tspan class="fn" data-hash="0" hash="1">unwrap()</tspan>;
} 
