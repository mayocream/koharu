# ProjectSummary

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**id** | **String** | Stable identifier — the `.khrproj` directory basename (without the extension). Clients address projects by this. | 
**name** | **String** |  | 
**path** | **String** | Absolute filesystem path. Informational; clients never need to pass it back in — they use `id`. | 
**updated_at_ms** | Option<**u64**> | Last modification time of the project directory on disk (ms since UNIX epoch). Used for \"recent projects\" ordering. | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


