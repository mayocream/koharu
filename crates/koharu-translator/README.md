# Koharu Translator

koharu-translator provides one segment-preserving translation interface for
local GGUF models and hosted providers.

Model metadata is provider-neutral: each model has a stable provider ID, a
display name, and (for local models) selectable GGUF quantizations. Every
provider module owns its connection config, defaults, request mapping, and
model catalog or discovery.

The crate owns only provider connectivity. Provider settings such as base URLs
are persisted under `[providers]`; credentials remain in the platform keychain.
Translation model selection, target language, instructions, and generation
options belong to the caller. `koharu-pipeline` owns those values as the
configuration of its translation processor.

`Translator` is an execution engine. A caller supplies a `ModelSelection`,
`GenerationConfig`, and `TranslationRequest` for each operation. The engine
keeps the selected local model resident when possible and reads live provider
connection settings without owning workflow configuration.
