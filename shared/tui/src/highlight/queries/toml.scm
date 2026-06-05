; Minimal TOML highlight query.

(comment) @comment

(string) @string

(integer) @number
(float) @number
(boolean) @constant
(local_date) @constant
(local_time) @constant
(local_date_time) @constant
(offset_date_time) @constant

(bare_key) @variable
(quoted_key) @variable

[ "=" ] @operator
[ "[" "]" "{" "}" "," "." ] @punctuation
