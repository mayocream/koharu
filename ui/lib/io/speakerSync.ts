import type { Scene } from '@/lib/api/schemas'
import { applyOp } from '@/lib/io/scene'
import { ops } from '@/lib/ops'

/** Iterates through all pages in the scene and batch patches text nodes where speaker === oldName with newName (null allowed). */
export async function renameSpeakerInScene(
  scene: Scene,
  oldName: string,
  newName: string | null,
): Promise<void> {
  for (const page of Object.values(scene.pages)) {
    for (const [nodeId, node] of Object.entries(page.nodes)) {
      if (!('text' in node.kind)) continue
      if (node.kind.text.speaker !== oldName) continue

      await applyOp(
        ops.updateNode(page.id, nodeId, {
          data: { text: { speaker: newName } } as never,
        }),
      )
    }
  }
}