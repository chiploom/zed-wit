# WIT formatting query

`wit.scm` is derived from Topiary revision
`96b6f425643628cec9b36fb287f15c61c9d05ccd`,
`topiary-queries/queries/wit/formatting.scm`.

Source: https://github.com/topiary/topiary/blob/96b6f425643628cec9b36fb287f15c61c9d05ccd/topiary-queries/queries/wit/formatting.scm

The published `topiary-queries` 0.7.3 WIT query expects URI tokens directly under
`package_decl`; the pinned canonical grammar places them under `decl_head`.
The maintained upstream query supports that change and nested packages. The
formatter uses this query with the published Topiary engine 0.7.3. The upstream
MIT license is retained in `LICENSE.topiary`.

Local compatibility fixes: append line breaks after record, enum, flags, and
variant declarations, and insert their trailing field commas unconditionally.
Their bodies always format as multiline; the original input-sensitive trailing
comma rule deferred insertion until a second pass, breaking idempotence for
initially single-line bodies. Use/include names retain their original conditional
comma rule. These changes are covered by formatter regression tests.
