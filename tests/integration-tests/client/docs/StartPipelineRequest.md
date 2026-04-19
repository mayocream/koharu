# StartPipelineRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**default_font** | Option<**String**> |  | [optional]
**pages** | Option<**Vec<uuid::Uuid>**> | `None` → whole project, `Some(pages)` → just those pages. | [optional]
**region** | Option<[**models::Region**](Region.md)> | Optional bounding-box hint for inpainter engines (repair-brush). | [optional]
**steps** | **Vec<String>** | Engine ids (`inventory::submit!` ids) to run in order. | 
**system_prompt** | Option<**String**> |  | [optional]
**target_language** | Option<**String**> |  | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


