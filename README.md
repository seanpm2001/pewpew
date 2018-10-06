# Pewpew

## TODOs
- BUG: CHECK IF THIS IS STILL HAPENNING an endpoint dependent upon a response provider deadlocks if they're not both going the same speed
- in stats change `time` to `bucket_time` and add `start_time` and `end_time`
- BUG: scale ups and scale downs are not generating the expected amount of load
- allow statuses on response providers to have more than one entry, IE: `4xx or 5xx`
- Add additional types of transforms: substring, json path extraction, unnest
- add options to `Peek` provider: include outHeaders, log to file (each error gets new file, save all results in directory),
      or should these be included in the results directory
- add config options: for client: request timeout, standard headers, keepalive time; for providers: buffer size
- add `files` body provider
- update `mod_interval` code so that multiple `scale_fn`s can be added so that it handles the transition from
      one fn to the next, rather than using `Stream::chain`. This is important because, currently, if a
      provider value is delayed for a long period of time, it will start on the next `mod_interval` even
      though enough time may have passed that it should skip several `mod_interval`s
- url encode url templates (maybe just a helper)
- add more tests - unit and integration - get code coverage
- create a mechanism to create aliases so a request could get multiple values from the same provider
      also have a way to get an array of x values from a provider
- resolve static providers before creating endpoint stream
- add more configuration to file provider. random order; format: string, json, csv
- create template helpers to extract fields from csv
- add in ability to log connection timeouts, connection errors, etc
- track system health (sysinfo crate) perhaps event loop latency and determine if system is overloaded
- ensure RTTs are accurate. Is there any queueing of requests if waiting for an available socket???
- add in machine clustering. Machines should open up a secure connection using a PSK
- create custom executor with priorities. system monitoring and logging should be high priority
      generating load should be low priority
- are there cases a request provider (without load_pattern) should drop a value rather than buffering it
      in the channel or event loop???
- create stats viewer (raw html + js + css - no server)
- allow load_patterns/config to change while a test is running
- verbose output mode (show when a request is made, when a response is received with RTT)
- add test monitoring. Possibly use tui for terminal output. Have webserver which will display a dashboard
- make the check for load_pattern happen during deserialization so a parse error with line numbers
      can be propogated