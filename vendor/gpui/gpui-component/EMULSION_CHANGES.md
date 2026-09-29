# Emulsion changes to gpui-component 0.6.4

These changes remain under Apache-2.0. Original source notices are retained.

- `src/menu/context_menu.rs`: the menu's DismissEvent subscription captures
  its shared state weakly. The shared state stores that subscription, and a
  `Subscription` keeps its callback alive, so the strong capture formed an `Rc`
  cycle that leaked the state and its `PopupMenu` once the trigger element was
  gone (e.g. its window closed while a menu had been opened).

Archive and upstream revision information are in `../UPSTREAM.json`.
