# \DefaultApi

All URIs are relative to *http://localhost*

Method | HTTP request | Description
------------- | ------------- | -------------
[**add_image_layer**](DefaultApi.md#add_image_layer) | **POST** /pages/{id}/image-layers | 
[**apply_command**](DefaultApi.md#apply_command) | **POST** /history/apply | 
[**cancel_operation**](DefaultApi.md#cancel_operation) | **DELETE** /operations/{id} | 
[**clear_provider_secret**](DefaultApi.md#clear_provider_secret) | **DELETE** /config/providers/{id}/secret | Clear a provider's keyring secret. The provider entry itself is kept.
[**create_pages**](DefaultApi.md#create_pages) | **POST** /pages | 
[**create_project**](DefaultApi.md#create_project) | **POST** /projects | 
[**delete_current_llm**](DefaultApi.md#delete_current_llm) | **DELETE** /llm/current | 
[**delete_current_project**](DefaultApi.md#delete_current_project) | **DELETE** /projects/current | 
[**events**](DefaultApi.md#events) | **GET** /events | 
[**export_current_project**](DefaultApi.md#export_current_project) | **POST** /projects/current/export | 
[**fetch_google_font**](DefaultApi.md#fetch_google_font) | **POST** /google-fonts/{family}/fetch | 
[**get_blob**](DefaultApi.md#get_blob) | **GET** /blobs/{hash} | 
[**get_catalog**](DefaultApi.md#get_catalog) | **GET** /llm/catalog | 
[**get_config**](DefaultApi.md#get_config) | **GET** /config | 
[**get_current_llm**](DefaultApi.md#get_current_llm) | **GET** /llm/current | 
[**get_engine_catalog**](DefaultApi.md#get_engine_catalog) | **GET** /engines | 
[**get_google_font_file**](DefaultApi.md#get_google_font_file) | **GET** /google-fonts/{family}/{file} | 
[**get_google_fonts_catalog**](DefaultApi.md#get_google_fonts_catalog) | **GET** /google-fonts | 
[**get_meta**](DefaultApi.md#get_meta) | **GET** /meta | 
[**get_page_thumbnail**](DefaultApi.md#get_page_thumbnail) | **GET** /pages/{id}/thumbnail | 
[**get_scene_bin**](DefaultApi.md#get_scene_bin) | **GET** /scene.bin | 
[**get_scene_json**](DefaultApi.md#get_scene_json) | **GET** /scene.json | 
[**import_project**](DefaultApi.md#import_project) | **POST** /projects/import | 
[**list_fonts**](DefaultApi.md#list_fonts) | **GET** /fonts | 
[**list_projects**](DefaultApi.md#list_projects) | **GET** /projects | 
[**patch_config**](DefaultApi.md#patch_config) | **PATCH** /config | 
[**put_current_llm**](DefaultApi.md#put_current_llm) | **PUT** /llm/current | 
[**put_current_project**](DefaultApi.md#put_current_project) | **PUT** /projects/current | 
[**put_mask**](DefaultApi.md#put_mask) | **PUT** /pages/{id}/masks/{role} | Upsert the `Mask { role }` node on a page with the raw image bytes in the body. Emits `Op::UpdateNode` if a mask of that role exists, else `Op::AddNode`. Used by the repair-brush / segment-edit flow; the follow-up localized inpaint is a separate `POST /pipelines` call.
[**redo**](DefaultApi.md#redo) | **POST** /history/redo | 
[**set_provider_secret**](DefaultApi.md#set_provider_secret) | **PUT** /config/providers/{id}/secret | Save (or overwrite) the keyring secret for a provider. Creates the provider entry in `config.providers` if it didn't exist. `PUT` because setting the secret is idempotent for the same body.
[**start_download**](DefaultApi.md#start_download) | **POST** /downloads | 
[**start_pipeline**](DefaultApi.md#start_pipeline) | **POST** /pipelines | 
[**undo**](DefaultApi.md#undo) | **POST** /history/undo | 



## add_image_layer

> models::AddImageLayerResponse add_image_layer(id)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **uuid::Uuid** | Page id | [required] |

### Return type

[**models::AddImageLayerResponse**](AddImageLayerResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: multipart/form-data
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## apply_command

> models::HistoryResult apply_command(op)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**op** | [**Op**](Op.md) |  | [required] |

### Return type

[**models::HistoryResult**](HistoryResult.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## cancel_operation

> cancel_operation(id)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **String** | Operation id | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## clear_provider_secret

> clear_provider_secret(id)
Clear a provider's keyring secret. The provider entry itself is kept.

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **String** | Provider id | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## create_pages

> models::CreatePagesResponse create_pages()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::CreatePagesResponse**](CreatePagesResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: multipart/form-data
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## create_project

> models::ProjectSummary create_project(create_project_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**create_project_request** | [**CreateProjectRequest**](CreateProjectRequest.md) |  | [required] |

### Return type

[**models::ProjectSummary**](ProjectSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## delete_current_llm

> delete_current_llm()


### Parameters

This endpoint does not need any parameter.

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## delete_current_project

> delete_current_project()


### Parameters

This endpoint does not need any parameter.

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## events

> models::AppEvent events()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::AppEvent**](AppEvent.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## export_current_project

> export_current_project(export_project_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**export_project_request** | [**ExportProjectRequest**](ExportProjectRequest.md) |  | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/octet-stream

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## fetch_google_font

> fetch_google_font(family)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**family** | **String** | Google Fonts family name | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_blob

> get_blob(hash)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**hash** | **String** | Blake3 hash of the blob | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/octet-stream

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_catalog

> models::LlmCatalog get_catalog()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::LlmCatalog**](LlmCatalog.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_config

> models::AppConfig get_config()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::AppConfig**](AppConfig.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_current_llm

> models::LlmState get_current_llm()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::LlmState**](LlmState.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_engine_catalog

> models::EngineCatalog get_engine_catalog()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::EngineCatalog**](EngineCatalog.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_google_font_file

> get_google_font_file(family, file)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**family** | **String** | Google Fonts family name | [required] |
**file** | **String** | Font filename | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: font/ttf

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_google_fonts_catalog

> models::GoogleFontCatalog get_google_fonts_catalog()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::GoogleFontCatalog**](GoogleFontCatalog.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_meta

> models::MetaInfo get_meta()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::MetaInfo**](MetaInfo.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_page_thumbnail

> get_page_thumbnail(id)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **uuid::Uuid** | Page id | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: image/webp

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_scene_bin

> get_scene_bin()


### Parameters

This endpoint does not need any parameter.

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/octet-stream

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## get_scene_json

> models::SceneSnapshot get_scene_json()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::SceneSnapshot**](SceneSnapshot.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## import_project

> models::ProjectSummary import_project()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ProjectSummary**](ProjectSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/zip
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## list_fonts

> Vec<models::FontFaceInfo> list_fonts()


### Parameters

This endpoint does not need any parameter.

### Return type

[**Vec<models::FontFaceInfo>**](FontFaceInfo.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## list_projects

> models::ListProjectsResponse list_projects()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::ListProjectsResponse**](ListProjectsResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## patch_config

> models::AppConfig patch_config(config_patch)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**config_patch** | [**ConfigPatch**](ConfigPatch.md) |  | [required] |

### Return type

[**models::AppConfig**](AppConfig.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## put_current_llm

> put_current_llm(llm_load_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**llm_load_request** | [**LlmLoadRequest**](LlmLoadRequest.md) |  | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## put_current_project

> models::ProjectSummary put_current_project(open_project_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**open_project_request** | [**OpenProjectRequest**](OpenProjectRequest.md) |  | [required] |

### Return type

[**models::ProjectSummary**](ProjectSummary.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## put_mask

> models::PutMaskResponse put_mask(id, role)
Upsert the `Mask { role }` node on a page with the raw image bytes in the body. Emits `Op::UpdateNode` if a mask of that role exists, else `Op::AddNode`. Used by the repair-brush / segment-edit flow; the follow-up localized inpaint is a separate `POST /pipelines` call.

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **uuid::Uuid** | Page id | [required] |
**role** | [**MaskRole**](MaskRole.md) | Mask role (segment|brushInpaint) | [required] |

### Return type

[**models::PutMaskResponse**](PutMaskResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: image/png
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## redo

> models::HistoryResult redo()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::HistoryResult**](HistoryResult.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## set_provider_secret

> set_provider_secret(id, provider_secret_request)
Save (or overwrite) the keyring secret for a provider. Creates the provider entry in `config.providers` if it didn't exist. `PUT` because setting the secret is idempotent for the same body.

### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**id** | **String** | Provider id | [required] |
**provider_secret_request** | [**ProviderSecretRequest**](ProviderSecretRequest.md) |  | [required] |

### Return type

 (empty response body)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: Not defined

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## start_download

> models::StartDownloadResponse start_download(start_download_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**start_download_request** | [**StartDownloadRequest**](StartDownloadRequest.md) |  | [required] |

### Return type

[**models::StartDownloadResponse**](StartDownloadResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## start_pipeline

> models::StartPipelineResponse start_pipeline(start_pipeline_request)


### Parameters


Name | Type | Description  | Required | Notes
------------- | ------------- | ------------- | ------------- | -------------
**start_pipeline_request** | [**StartPipelineRequest**](StartPipelineRequest.md) |  | [required] |

### Return type

[**models::StartPipelineResponse**](StartPipelineResponse.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: application/json
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)


## undo

> models::HistoryResult undo()


### Parameters

This endpoint does not need any parameter.

### Return type

[**models::HistoryResult**](HistoryResult.md)

### Authorization

No authorization required

### HTTP request headers

- **Content-Type**: Not defined
- **Accept**: application/json

[[Back to top]](#) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to Model list]](../README.md#documentation-for-models) [[Back to README]](../README.md)

