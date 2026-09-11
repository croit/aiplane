import type { ChatAsset } from './api';
import type { ChatAttachment } from './chat-protocol';

export function canonicalAttachments(
	attachments: ChatAttachment[],
	assets: ChatAsset[],
	turnId: string
): ChatAttachment[] {
	const seen = new Set<string>();
	return attachments.flatMap((attachment) => {
		const filenameMatches = assets.filter((asset) => asset.filename === attachment.filename);
		const asset =
			filenameMatches.find((candidate) => candidate.turn_id === turnId) ??
			filenameMatches.find((candidate) => candidate.url === attachment.url) ??
			(filenameMatches.length === 1 ? filenameMatches[0] : undefined);
		const canonical = asset
			? { ...attachment, mime: asset.mime, size: asset.size, url: asset.url }
			: attachment;
		if (seen.has(canonical.url)) return [];
		seen.add(canonical.url);
		return [canonical];
	});
}
