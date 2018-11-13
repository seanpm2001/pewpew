# Pewpew

## Config file
The pewpew executable requires a single parameter specifying the path to a load test config file. A config file is yaml with a particular schema. Here's a simple example:

```yaml
load_pattern:
  - linear:
      to: 100%
      over: 5m
  - linear:
      to: 100%
      over: 2m
endpoints:
  - method: GET
    url: http://127.0.0.1:8080/foo
    peak_load: 42hpm
    headers:
      Accept: text/plain
  - method: GET
    url: http://127.0.0.1:8080/bar
    headers:
      Accept-Language: en-us
      Accept: application/json
    peak_load: 15hps
```

The above config file tells pewpew to hit two HTTP endpoints with particular loads. The entire test will last seven minutes where the first five minutes will be scaling up to "100%" and the last two minutes will stay steady at "100%". For the first endpoint "100%" means 42 hits per minute and for the second it means 15 hits per second.

A config file can have four main sections.

### load_pattern <sub><sup>*Optional*</sup></sub>
---
<pre>
load_pattern:
  - <i>load_pattern_type</i>
      [parameters]
</pre>

This section defines the "shape" that the generated traffic will take over the course of the test. Individual endpoints can choose to specify their own `load_pattern` (see the [`endpoints` section](#endpoints)). If a root level `load_pattern` is not specified then each endpoint *must* specify its own `load_pattern`.

`load_pattern` is an array of *load_pattern_type*s specifying how generated traffic for a segment of the test will scale up, down or remain steady. Currently the only *load_pattern_type* supported is `linear`.

Example:
```yaml
load_pattern:
  - linear:
      to: 100%
      over: 5m
  - linear:
      to: 100%
      over: 2m
```

#### linear
---
The linear *load_pattern_type* allows generated traffic to increase or decrease linearly. There are three parameters which can be specified for each linear segment:

- **`from`** <sub><sup>*Optional*</sup></sub> - The starting point this segment will scale from, specified as a percentage. Defaults to `0%` if the current segment is the first entry in `load_pattern`, or the `to` value in the previous segment.

  A valid percentage is any unsigned number, integer or decimal, immediately followed by the percent symbol (`%`). Percentages can exceed `100%` but cannot be negative. For example `15.25%` or `150%`. 
- **`to`** - The end point this segment should scale to, specified as a percentage.
- **`over`** - The duration for how long this segment should last.
  A duration is an integer followed by an optional space and a string value indicating the time unit. Hours can be specified with "h", "hr", "hrs", "hour", or "hours", minutes with "m", "min", "mins", "minute", or "minutes", and seconds with "s", "sec", "secs", "second", or "seconds".

  Examples:

  `1h` = 1 hour

  `30 minutes` = 30 minutes
  
  Multiple duration pieces can be chained together to form more complex durations.

  Examples:

  `1h45m30s` = 1 hour, 45 minutes and 30 seconds

  `4 hrs 15 mins` = 4 hours and 15 minutes

  As seen in the above examples, an optional space can be used to delimit the individual duration pieces.

### providers <sub><sup>*Optional*</sup></sub>
---
<pre>
providers:
  <i>provider_name</i>:
    <i>provider_type</i>:
      [parameters]
</pre>

Providers are the means of providing data to an endpoint, including using data from the response of one endpoint in the request of another. The way providers handle data can be thought of as a FIFO queue. Every provider has an internal buffer which has a soft limit on how many items can be stored.

A *provider_name* is any string except for "request" and "response", which are reserved.

Example:
```yaml
providers:
  - session:
    - endpoint:
      - auto_return: force
  - username:
    - file:
      path: "usernames.csv"
      repeat: true
```

Every *provider_type*, except `static` and `static-list`, supports the following two optional parameters:
- **`auto_return`** <sub><sup>*Optional*</sup></sub> - This parameter specifies that when this provider is used and an individual endpoint call concludes, the value it got from this provider should be sent back to the provider. Valid options for this parameter are `block`, `force`, and `if_not_full`.

  `block` indicates that if the provider's buffer is full, further endpoint calls will be blocked until the value can be returned.
  
  `force` indicates that the value will be returned to the provider regardless of whether its buffer is "full". This can make a provider's buffer exceed its soft limit.
  
  `if_not_full` indicates that the value will be returned to the provider only if the provider is not full.
- **`buffer`** <sub><sup>*Optional*</sup></sub> - Specifies the soft limit for a provider's buffer. This can be indicated with an integer greater than zero or the value `auto`. The value `auto` indicates that if the provider's buffer becomes empty it will automatically increase the buffer size to help prevent the the provider from being empty. Defaults to `auto`.

There are four *provider_type*s:

#### file
The `file` *provider_type* reads data from a file. Every line in the file is read as a value. In the future, the ability to specify the format of the data (csv, json, etc) may be implemented. A `file` provider has the following parameters:

- **path** - A string value indicating the path to the file on the file system. Currently, a relative path is interpreted as being relative to the current working directory where `pewpew` was executed from. In the future this may be changed to be relative to the location of the config file.
- **repeat** - <sub><sup>*Optional*</sup></sub> A boolean value which when `true` indicates when the provider `file` provider gets to the end of the file it should start back at the beginning. Defaults to `false`.

#### response
Unlike other *provider_type*s `response` does not automatically receive data from a source. Instead a `response` provider is available to be a "sink" for data originating from an HTTP response. The `response` provider has no additional parameters beyond `auto_return` and `buffer`.

#### static
The `static` *provider_type* is used for having a single pre-defined value used throughout a test. A `static` provider will make copies of the value every time a value is required from the provider. When defining a `static` provider the only parameter is the literal value which should be used.

For example:
```yaml
providers:
  foo:
    static: bar
```

creates a single `static` provider named `foo` where the value is the string "bar".

More complex values are automatically interpreted as JSON so the following:
```yaml
providers:
  bar:
    static:
      a: 1
      b: 2
      c: 3
```

creates a `static` provider named `bar` where the value is equivalent to the JSON `{"a": 1, "b": 2, "c": 3}`.

#### static_list
The `static_list` *provider_type* is like the `static` *provider_type* except an array of values can be specified and the provider will iterate infinitely over the array using each element as the value to be provided.

The following:
```yaml
providers:
  foo:
    static_list:
      - 123
      - 456
      - 789
```

creates a `static_list` provider named `foo` where the first value provided will be `123`, the second `456`, third `789` then for subsequent values it will start over at the beginning.

### loggers <sub><sup>*Optional*</sup></sub>
---
<pre>
loggers:
  <i>logger_name</i>:
    [select: <i>select_piece</i>]
    [for_each: <i>for_each_piece</i>]
    [where: <i>where_piece</i>]
    to: <i>filename</i> | stderr | stdout
    [pretty: <i>boolean</i>]
    [limit: <i>integer</i>]
</pre>
Loggers provide a means of logging data to a file, stderr or stdout. Any string can be used for *logger_name*.

There are two types of loggers: plain loggers which have data logged to them by explicitly referencing them within an `endpoints`.`log` section, and global loggers which are evaluated for every endpoint response and cannot be explicitly specified within an `endpoints`.`log` section.

Loggers support the following parameters:
- **`select`** <sub><sup>*Optional*</sup></sub> - When specified, the logger becomes a global logger. See the [`endpoints`.`provides` section](#provides) for details on how to define a `select_piece`.
- **`for_each`** <sub><sup>*Optional*</sup></sub> - Used in conjunction with `select` on global loggers.  See the [`endpoints`.`provides` section](#provides) for details on how to define a `for_each_piece`.
- **`where`** <sub><sup>*Optional*</sup></sub> - Used in conjunction with `select` on global loggers.  See the [`endpoints`.`provides` section](#provides) for details on how to define a `where_piece`.
- **`to`** - A string specifying where this logger will send its data. Values of "stderr" and "stdout" will log data to the respective process streams and any other string will log to a file with that name. Currently files are created in the current working directory where the pewpew process was launched from. When a file is specified, the file will be created if it does not exist or will be truncated if it already exists.
- **`pretty`** <sub><sup>*Optional*</sup></sub> - A boolean that when `true` the value logged will have added whitespace for readability. Defaults to `false`.
- **`limit`** <sub><sup>*Optional*</sup></sub> - An unsigned integer which indicates the logger will only log *n* values.

Example:
```yaml
loggers:
  http_errors:
    select:
      request:
        - request["start-line"]
        - request.headers
        - request.body
      response:
        - response["start-line"]
        - response.headers
        - response.body
    where: response.status >= 400
    limit: 5
    to: http_err.log
    pretty: true
```

Creates a global logger which will log to the file "http_err.log" the request and response of the first five requests which have an HTTP status of 400 or greater.

### endpoints
---
<pre>
endpoints:
  - [declare: <i>declare_section</i>]
    [headers: <i>headers</i>]
    [body: <i>body</i>]
    [load_pattern: <i>load_pattern_section</i>]
    [method: <i>method</i>]
    [peak_load: <i>peak_load</i>]
    [stats_id: <i>stats_id</i>]
    url: <i>url</i>
    [provides: <i>provides_section</i>]
    [logs: <i>logs_section</i>]
</pre>
The `endpoints` section declares what HTTP endpoints will be called during a test.

- **`declare`** <sub><sup>*Optional*</sup></sub> - See the [declare section](#declare)
- **`headers`** <sub><sup>*Optional*</sup></sub> - Key/value string pairs which specify the headers which should be used for the request. Values can be interpolated with names of providers. For example:

  ```yaml
  endpoints:
    url: https://localhost/foo/bar
    headers:
      Authorization: Bearer {{sessionId}}
  ```
  specifies that an "Authorization" header will be sent with the request with a value of "Bearer " followed by a value coming from a provider named "sessionId".
- **`body`** <sub><sup>*Optional*</sup></sub> - A string value indicating the body that should be sent with the request.
- **`load_pattern`** <sub><sup>*Optional*</sup></sub> - See the [load_pattern section](#load_pattern-optional)
- **`method`** <sub><sup>*Optional*</sup></sub> - A string representation for a valid HTTP method verb. Defaults to `GET`
- **`peak_load`** <sub><sup>*Optional*</sup></sub> - A string representing what the "peak load" for this endpoint should be. The term "peak load" represents what a `load_pattern` value of `100%` represents for this endpoint. Note: that a `load_pattern` can go higher than `100%`, so a `load_pattern` of `200%`, for example, would mean it would go double the defined `peak_load`.

  A valid `load_pattern` is an unsigned integer followed by an optional space and the string "hpm" (meaning "hits per minute") or "hps" (meaning "hits per second").

  Examples:

  `50hpm` - 50 hits per minute

  `300 hps` - 300 hits per second

  TODO: note when `peak_load` is required
- **`stats_id`** <sub><sup>*Optional*</sup></sub> - Key/value string pairs indicating additional keys which will be added to an endpoint's stats identifier. A stats identifier is a series of key/value pairs used to identify each endpoint. This makes it easier to distinguish endpoints in a test with several endpoints. By default every endpoint has a default stats identifier of the HTTP method and the immutable parts of the url.

  In most cases it is not nececessary to specify additional key/value pairs for the `stats_id`, but it can be helpful if multiple endpoints have the same url and method pair and the default `stats_id` is not descriptive enough.
- **`url`** - A string value specifying the fully qualified url to the endpoint which will be requested.
- **`provides`** <sub><sup>*Optional*</sup></sub> - See the [provides section](#provides)
- **`logs`** <sub><sup>*Optional*</sup></sub> - See the [logs section](#logs)

#### Referencing Providers
TODO examples of provider interpolation and helpers

#### declare
<pre>
declare:
  <i>name</i>: <i>provider_name</i> | collect(<i>collect_args</i>)
</pre>
A *declare_section* provides the ability to select multiple values from a single provider. Without using a *declare_section*, multiple references to a provider will only select a single value. For example, in:

```yaml
endpoints:
  - method: PUT
    url: https://127.0.0.1/ship/{{shipId}}/speed
    body: '{"shipId":"{{shipId}}","kesselRunTime":75}'
```

both references to the provider `shipId` will resolve to the same value, which is desired in many cases.

The *declare_section* is in the format of key/value string pairs. Every key can function as a provider and can be interpolated just as a provider would be. Values can be in one of two formats:
1) a string which is a reference to a provider
2) a call to the `collect` function. The `collect` function "collects" multiple values from a provider into an array. `collect` can be called with two or three arguments in the format <code>collect(*n*, *provider_name*)</code> or <code>collect(*min*, *max*, *provider_name*)</code>. The two argument form creates an array of size *n* with values from a provider. The three argument form creates an array with a randomly selected size between *min* and *max* (both *min* and *max* are inclusive) with values from a provider.

Examples:
```yaml
endpoints:
  - declare:
      shipIds: collect(3, 5, shipId)
    method: DELETE
    url: https://127.0.0.1/ships
    body: '{"shipIds":{{shipIds}}}'
```
Calls the endpoint `DELETE /ships` where the body is interpolated with an array of ship ids. `shipIds` will have a length between three and five.

```yaml
endpoints:
  - declare:
      destroyedShipId: shipId
    method: PUT
    url: https://127.0.0.1/ship/{{shipId}}/destroys/{{destroyedShipId}}
```
Calls `PUT` on an endpoint where `shipId` and `destroyedShipId` are interpolated to different values.


#### provides
<pre>
provides:
  <i>provider_name</i>:
    [send: block | force | if_not_full]
    select: <i>select_piece</i>
    [for_each: <i>for_each_piece</i>]
    [where: <i>where_piece</i>]
</pre>
The *provides_section* is how data can be sent into a provider from an HTTP response. *provider_name* is a reference to a provider which must be declared in the root [`providers` section](#providers-optional).

Sending data into a provider is done with a SQL-like syntax.

- **`send`** <sub><sup>*Optional*</sup></sub> - See the `auto_return` parameter in the [`providers` section](#providers-optional).
- **`select`** - Determines the shape of the data sent into the provider.
- **`for_each`** <sub><sup>*Optional*</sup></sub> - Evaluates `select` for each element in an array or arrays.
- **`where`** <sub><sup>*Optional*</sup></sub> - Allows conditionally sending data into a provider based on a predicate.

TODO examples and more details. Include use of `json_path` and `repeat`

#### logs
TODO