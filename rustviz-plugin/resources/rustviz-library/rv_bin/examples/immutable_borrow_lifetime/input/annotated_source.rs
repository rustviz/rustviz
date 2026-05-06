fn main() {
  let <tspan data-hash="1">string1</tspan> = <tspan class="fn" data-hash="0" hash="6">String::from</tspan>("abcd");
  let <tspan data-hash="2">string2</tspan> = <tspan class="fn" data-hash="0" hash="6">String::from</tspan>("xyz");
  
  let <tspan data-hash="5">result</tspan> = <tspan class="fn" data-hash="0" hash="7">longest</tspan>(&amp;<tspan data-hash="1">string1</tspan>, &amp;<tspan data-hash="2">string2</tspan>);
  <tspan class="fn" data-hash="0" hash="8">println!</tspan>("The longest string is {}", <tspan data-hash="5">result</tspan>);
}

fn <tspan class="fn" data-hash="0" hash="7">longest</tspan>&lt;'a &gt;(<tspan data-hash="3">x</tspan>: &amp;'a String, <tspan data-hash="4">y</tspan>: &amp;'a String) -> &amp;'a String {
    if <tspan data-hash="3">x</tspan>.len() > <tspan data-hash="4">y</tspan>.len() {
      <tspan data-hash="3">x</tspan>
    } else {
      <tspan data-hash="4">y</tspan>
    }
}
