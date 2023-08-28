/* --- BEGIN Variable Definitions ---
LifetimeVars &mut request_queue; LifetimeVars &mut available_resource; LifetimeVars request_halfway; LifetimeBind &mut read_request -> &mut request_queue; LifetimeBind &mut update_request -> &mut request_queue; LifetimeBind &mut delete_request -> &mut request_queue
--- END Variable Definitions --- */
fn process_requests<'i,'a>(queue: &'i mut VecDeque<&'i mut Request<'i>>, max_process_unit: &'a mut u32) -> Option<&'i mut Request<'i>>{
    loop {
        let front_request: Option<&mut Request> = queue.pop_front();
        if let Some(request) = front_request{
            // if current max_process_unit is greater than current requests
            if request.num_request_left <= max_process_unit{
                println!("Served #{} of {} requests.", request.num_request_left, request.request_type.to_string());
                // decrement the amount of resource spent on this request
                *max_process_unit = *max_process_unit - *request.num_request_left;
                // signify this request has been processed
                *request.num_request_left = 0;
            }
            // not enough
            else{
                // process as much as we can
                *request.num_request_left = *request.num_request_left - *max_process_unit;
                // sad, no free resource anymore
                *max_process_unit = 0;
                // enqueue the front request back to queue, hoping someone will handle it...
                // queue.push_front(request);
                return Option::Some(request);
            }
            //
        }
        else {
            // no available request to process, ooh-yeah!
            return Option::None;

        }
    }
}

fn main() {
    let mut request_queue: VecDeque<&mut Request> = VecDeque::new();
    // generating some requests
    let mut reads_cnt: u32 = 20;
    let mut read_request: Request = Request::new(&mut reads_cnt, RequestType::READ);
    // enqueue read requests
    request_queue.push_back(&mut read_request);
    let mut updates_cnt: u32 = 30;
    let mut update_request: Request = Request::new(&mut updates_cnt, RequestType::UPDATE);
    request_queue.push_back(&mut update_request);
    // enqueue update requests
    let mut deletes_cnt: u32 =50;
    let mut delete_request: Request = Request::new(&mut deletes_cnt, RequestType::DELETE);
    request_queue.push_back(&mut delete_request);
    // enqueue delete requests

    // ..., process requests
    let mut available_resource: u32 = 10;
    let request_halfway = process_requests(&mut request_queue, &mut available_resource); // !{ Lifetime(<FUNC: process_requests>[&mut request_queue{51:51}][&mut read_request{39:56}][&mut update_request{42:56}][&mut delete_request{46:56}][&mut available_resource{51:51}]->[request_halfway{51:52}] )}
    if let Some(req) = request_halfway {
        println!("#{} of {} requests are left unprocessed!", req.num_request_left, req.request_type.to_string());
    }
    println!("there are #{} free resource left.", available_resource);
}