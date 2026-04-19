# ImageData

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**blob** | **String** | Hex-encoded blake3 hash of an immutable blob. | 
**name** | Option<**String**> |  | [optional]
**natural_height** | **u32** |  | 
**natural_width** | **u32** |  | 
**opacity** | Option<**f32**> |  | [optional]
**role** | [**models::ImageRole**](ImageRole.md) | Role tags differentiate source / inpainted / rendered / user-imported images. Role is immutable on an existing node — switching roles = delete + add. | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


