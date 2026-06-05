; Minimal Python highlight query.

[
  "and" "as" "assert" "async" "await" "break" "class" "continue" "def"
  "del" "elif" "else" "except" "finally" "for" "from" "global" "if"
  "import" "in" "is" "lambda" "nonlocal" "not" "or" "pass" "raise"
  "return" "try" "while" "with" "yield" "match" "case"
] @keyword

(true) @constant
(false) @constant
(none) @constant

(integer) @number
(float) @number

(string) @string

(comment) @comment

(decorator) @attribute

(function_definition name: (identifier) @function)
(call function: (identifier) @function)
(call function: (attribute attribute: (identifier) @function))

(class_definition name: (identifier) @type)

[
  "+" "-" "*" "/" "%" "**" "//" "=" "==" "!=" "<" ">" "<=" ">="
  "+=" "-=" "*=" "/=" "%=" "**=" "//=" "&" "|" "^" "<<" ">>"
  "->" ":=" "@"
] @operator

[ "(" ")" "[" "]" "{" "}" "," ":" ";" "." ] @punctuation

(identifier) @variable
