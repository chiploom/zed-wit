(package_decl (decl_head) @name) @item
(nested_package_definition (decl_head) @name) @item
(interface_item "interface" @context name: (id) @name) @item
(world_item "world" @context name: (id) @name) @item
(type_item "type" @context alias: (id) @name) @item
(record_item "record" @context name: (id) @name) @item
(flags_items "flags" @context name: (id) @name) @item
(enum_items "enum" @context name: (id) @name) @item
(variant_items "variant" @context name: (id) @name) @item
(resource_item "resource" @context name: (id) @name) @item
(record_field name: (id) @name) @item
(variant_case name: (id) @name) @item
(flags_field) @name @item
(enum_case) @name @item
(func_item name: (id) @name) @item
(resource_method name: (id) @name "static" @context) @item
(resource_method "constructor" @name) @item
(import_item "import" @context name: (id) @name) @item
(export_item "export" @context name: (id) @name) @item
(import_item "import" @context (use_path) @name) @item
(export_item "export" @context (use_path) @name) @item
