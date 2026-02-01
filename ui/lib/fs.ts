export async function pickFiles(
  types: FilePickerAcceptType[],
  multiple = false,
): Promise<File[]> {
  let handles: FileSystemFileHandle[]
  try {
    handles = await window.showOpenFilePicker({ multiple, types })
  } catch {
    return []
  }
  return Promise.all(handles.map((h) => h.getFile()))
}

export async function saveFile(
  blob: Blob,
  suggestedName: string,
): Promise<void> {
  let handle: FileSystemFileHandle
  try {
    handle = await window.showSaveFilePicker({ suggestedName })
  } catch {
    return
  }
  const writable = await handle.createWritable()
  await writable.write(blob)
  await writable.close()
}
