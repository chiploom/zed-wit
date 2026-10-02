(id) @variable

(decl_head (id) @type)
(use_path (id) @type)
(toplevel_use_item alias: (id) @type)
(interface_item name: (id) @type)
(world_item name: (id) @type)
(type_item alias: (id) @type)
(record_item name: (id) @type)
(resource_item name: (id) @type)
(flags_items name: (id) @type)
(enum_items name: (id) @type)
(variant_items name: (id) @type)
(ty (id) @type)
(handle (id) @type)
(use_names_item (id) @type)
(alias_item (id) @type)

(func_item name: (id) @function)
(resource_method name: (id) @function)
(resource_method "constructor" @constructor)
(import_item name: (id) @function (extern_type (func_type)))
(export_item name: (id) @function (extern_type (func_type)))
(import_item name: (id) @type (extern_type (body)))
(export_item name: (id) @type (extern_type (body)))
(import_item name: (id) @type (extern_type (use_path)))
(export_item name: (id) @type (extern_type (use_path)))
(named_type name: (id) @variable.parameter)
(record_field name: (id) @property)
(flags_field) @constant
(enum_case) @enum
(variant_case name: (id) @variant)

["package" "interface" "world" "use" "as" "include" "with"
 "import" "export" "type" "resource" "record" "flags" "enum"
 "variant" "func" "static" "async"] @keyword

["u8" "u16" "u32" "u64" "s8" "s16" "s32" "s64" "f32" "f64"
 "bool" "char" "string" "list" "tuple" "option" "result" "borrow"
 "map" "future" "stream"] @type.builtin

["since" "unstable" "deprecated" "external-id"] @attribute
["feature" "version"] @property
(unstable_gate feature: (id) @string.special)
(version) @string.special
(string_literal) @string
(uint) @number
"_" @constant.builtin
["=" "->"] @operator
[":" ";" "," "." "/"] @punctuation.delimiter
"@" @punctuation.special
["{" "}" "(" ")" "<" ">"] @punctuation.bracket

[(line_comment) (block_comment)] @comment
(line_comment (doc_comment)) @comment.doc
(block_comment (doc_comment)) @comment.doc

; Getter/setter sugar is accepted by wit-parser but not yet represented by the
; pinned Tree-sitter grammar. Tree-sitter may recover the accessor word either
; as a bare id or as an id nested in ty. Keep these overrides last so Zed's
; overlapping-highlight resolution prefers accessor semantics only here.
((ERROR
  (id) @keyword
  .
  "("
  .
  ")")
 (#eq? @keyword "get"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  ")")
 (#eq? @keyword "get"))

((ERROR
  (id) @keyword
  .
  "("
  .
  (id) @variable.parameter
  .
  ":")
 (#eq? @keyword "set"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  (id) @variable.parameter
  .
  ":")
 (#eq? @keyword "set"))

; User-defined setter parameter and getter return types can also be recovered as
; bare ids inside the error node. Builtin types continue to use the normal
; literal captures above.
((ERROR
  (id) @keyword
  .
  "("
  .
  (id) @variable.parameter
  .
  ":"
  .
  (id) @type)
 (#eq? @keyword "set"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  (id) @variable.parameter
  .
  ":"
  .
  (id) @type)
 (#eq? @keyword "set"))

((ERROR
  (id) @keyword
  .
  "("
  .
  ")"
  .
  "->"
  .
  (id) @type)
 (#eq? @keyword "get"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  ")"
  .
  "->"
  .
  (id) @type)
 (#eq? @keyword "get"))


; Builtins can be recovered as ids inside accessor error nodes. These rules
; intentionally come after the generic @type recovery so builtin styling wins.
((ERROR
  (id) @keyword
  .
  "("
  .
  (id) @variable.parameter
  .
  ":"
  .
  (id) @type.builtin)
 (#eq? @keyword "set")
 (#any-of? @type.builtin
  "u8" "u16" "u32" "u64" "s8" "s16" "s32" "s64" "f32" "f64"
  "bool" "char" "string" "list" "tuple" "option" "result" "borrow"
  "map" "future" "stream"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  (id) @variable.parameter
  .
  ":"
  .
  (id) @type.builtin)
 (#eq? @keyword "set")
 (#any-of? @type.builtin
  "u8" "u16" "u32" "u64" "s8" "s16" "s32" "s64" "f32" "f64"
  "bool" "char" "string" "list" "tuple" "option" "result" "borrow"
  "map" "future" "stream"))

((ERROR
  (id) @keyword
  .
  "("
  .
  ")"
  .
  "->"
  .
  (id) @type.builtin)
 (#eq? @keyword "get")
 (#any-of? @type.builtin
  "u8" "u16" "u32" "u64" "s8" "s16" "s32" "s64" "f32" "f64"
  "bool" "char" "string" "list" "tuple" "option" "result" "borrow"
  "map" "future" "stream"))

((ERROR
  (ty
    (id) @keyword)
  .
  "("
  .
  ")"
  .
  "->"
  .
  (id) @type.builtin)
 (#eq? @keyword "get")
 (#any-of? @type.builtin
  "u8" "u16" "u32" "u64" "s8" "s16" "s32" "s64" "f32" "f64"
  "bool" "char" "string" "list" "tuple" "option" "result" "borrow"
  "map" "future" "stream"))

; Accessor declaration names can also be recovered as ids or ty(id) nodes.
; Keep these rules last so they override generic variable/type captures and
; match the @function styling used by ordinary WIT function declarations.
((ERROR
  (id) @function
  .
  ":"
  .
  (id) @keyword
  .
  "(")
 (#any-of? @keyword "get" "set"))

((ERROR
  (id) @function
  .
  ":"
  .
  (ty
    (id) @keyword)
  .
  "(")
 (#any-of? @keyword "get" "set"))

((ERROR
  (ty
    (id) @function)
  .
  ":"
  .
  (id) @keyword
  .
  "(")
 (#any-of? @keyword "get" "set"))

((ERROR
  (ty
    (id) @function)
  .
  ":"
  .
  (ty
    (id) @keyword)
  .
  "(")
 (#any-of? @keyword "get" "set"))

