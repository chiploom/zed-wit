(interface_item (body "{" (_)* @class.inside "}")) @class.around
(world_item (body "{" (_)* @class.inside "}")) @class.around
(resource_item) @class.around
(resource_item (body "{" (_)* @class.inside "}"))
(record_item (body "{" (_)* @class.inside "}")) @class.around
(flags_items (body "{" (_)* @class.inside "}")) @class.around
(enum_items (body "{" (_)* @class.inside "}")) @class.around
(variant_items (body "{" (_)* @class.inside "}")) @class.around
(nested_package_definition "{" (_)* @class.inside "}") @class.around
(func_item) @function.around
(resource_method name: (id)) @function.around
(resource_method "constructor") @function.around
(import_item (extern_type (func_type))) @function.around
(export_item (extern_type (func_type))) @function.around
(line_comment)+ @comment.around
(block_comment) @comment.around
