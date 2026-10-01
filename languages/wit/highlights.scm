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
