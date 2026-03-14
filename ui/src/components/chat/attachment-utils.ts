const IMAGE_MIME_PREFIX = "image/";
const DEFAULT_PASTED_EXTENSION = "png";

export function isImageFile(file: File): boolean {
  return file.type.startsWith(IMAGE_MIME_PREFIX);
}

export function dedupeFiles(files: File[]): File[] {
  const seen = new Set<string>();
  const deduped: File[] = [];

  for (const file of files) {
    const signature = `${file.name}:${file.size}:${file.type}:${file.lastModified}`;
    if (seen.has(signature)) continue;
    seen.add(signature);
    deduped.push(file);
  }

  return deduped;
}

export function limitFiles(files: File[], maxAttachments: number): File[] {
  return files.slice(0, Math.max(maxAttachments, 0));
}

export function sanitizeIncomingAttachments(
  incoming: File[],
  maxAttachments: number,
): { accepted: File[]; rejected: File[] } {
  const dedupedImages = dedupeFiles(incoming.filter(isImageFile));
  const accepted = limitFiles(dedupedImages, maxAttachments);
  const acceptedSignatures = new Set(
    accepted.map((file) => `${file.name}:${file.size}:${file.type}:${file.lastModified}`),
  );
  const rejected = incoming.filter((file) => {
    if (!isImageFile(file)) return true;
    const signature = `${file.name}:${file.size}:${file.type}:${file.lastModified}`;
    return !acceptedSignatures.has(signature);
  });

  return { accepted, rejected };
}

export function fileListToArray(files: FileList | null): File[] {
  return files ? Array.from(files) : [];
}

export function clipboardImagesFromEvent(event: React.ClipboardEvent): File[] {
  return Array.from(event.clipboardData.items)
    .filter((item) => item.type.startsWith(IMAGE_MIME_PREFIX))
    .map((item, index) => {
      const file = item.getAsFile();
      if (!file) return null;

      const extension = file.type.split("/")[1] || DEFAULT_PASTED_EXTENSION;
      return new File([file], `pasted-image-${Date.now()}-${index}.${extension}`, {
        type: file.type,
        lastModified: Date.now(),
      });
    })
    .filter((file): file is File => file !== null);
}
