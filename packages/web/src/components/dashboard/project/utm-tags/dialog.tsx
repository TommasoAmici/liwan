import styles from "./utm-tags.module.css";

import type { FormEvent } from "react";
import { useId, useState } from "react";

import { api, useMutation } from "@/api";
import { Dialog } from "@/components/ui/dialog";
import { HelpTip } from "@/components/ui/help-tip";
import { CopyButton } from "@/components/ui/snippet";
import { createToast } from "@/components/ui/toast";
import type { UtmTag, UtmTagRequest } from "@/constants";
import { invalidateUtmTags } from "@/hooks/api";
import { buildUtmUrl } from "./build-url";

const emptyForm: UtmTagRequest = {
	displayName: "",
	url: "",
	utmSource: null,
	utmMedium: null,
	utmCampaign: null,
	utmContent: null,
	utmTerm: null,
};

const formFrom = (tag?: UtmTag | null): UtmTagRequest =>
	tag
		? {
				displayName: tag.displayName,
				url: tag.url,
				utmSource: tag.utmSource ?? null,
				utmMedium: tag.utmMedium ?? null,
				utmCampaign: tag.utmCampaign ?? null,
				utmContent: tag.utmContent ?? null,
				utmTerm: tag.utmTerm ?? null,
			}
		: emptyForm;

const errorMessage = (error: unknown, fallback: string) => {
	const message = (error as { message?: unknown } | null)?.message;
	return typeof message === "string" && message ? message : fallback;
};

const trimmed = (value: string | null | undefined) => {
	const text = value?.trim();
	return text ? text : null;
};

type UtmFieldKey = Exclude<keyof UtmTagRequest, "displayName" | "url">;

const utmDescriptions: Record<UtmFieldKey, string> = {
	utmSource:
		"Where the visit came from: a website, a newsletter, or an ad platform. Example: google, newsletter, linkedin.",
	utmMedium:
		"How the link was shared: email, social media, paid search, or another channel. Example: email, social, cpc.",
	utmCampaign:
		"The campaign or promotion the link belongs to. Use the same value on every link in a campaign. Example: spring_sale.",
	utmContent:
		"Which link or creative in the campaign was used, to tell two links apart. Example: header_button, sidebar.",
	utmTerm: "The paid keyword that brought the visit, mostly for search ads. Example: running_shoes.",
};

export const UtmTagDialog = ({
	projectId,
	tag,
	open,
	onOpenChange,
}: {
	projectId: string;
	tag?: UtmTag | null;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) => {
	const [form, setForm] = useState<UtmTagRequest>(() => formFrom(tag));
	const fieldId = useId();
	const preview = buildUtmUrl(form.url, {
		source: form.utmSource,
		medium: form.utmMedium,
		campaign: form.utmCampaign,
		content: form.utmContent,
		term: form.utmTerm,
	});

	const { mutate, isPending } = useMutation({
		mutationFn: (body: UtmTagRequest) =>
			tag
				? api["/api/dashboard/project/{project_id}/utm-tags/{tag_id}"].put({
						params: { project_id: projectId, tag_id: tag.id },
						json: body,
					})
				: api["/api/dashboard/project/{project_id}/utm-tags"].post({
						params: { project_id: projectId },
						json: body,
					}),
		onSuccess: async () => {
			await invalidateUtmTags(projectId);
			createToast(tag ? "UTM tag updated" : "UTM tag created", "success");
			onOpenChange(false);
		},
		onError: (error) =>
			createToast(errorMessage(error, tag ? "Failed to update UTM tag" : "Failed to create UTM tag"), "error"),
	});

	const submit = (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		if (isPending) return;
		mutate({
			displayName: form.displayName.trim(),
			url: form.url.trim(),
			utmSource: trimmed(form.utmSource),
			utmMedium: trimmed(form.utmMedium),
			utmCampaign: trimmed(form.utmCampaign),
			utmContent: trimmed(form.utmContent),
			utmTerm: trimmed(form.utmTerm),
		});
	};

	const field = (key: UtmFieldKey, label: string, id: string) => (
		<div className={styles.field}>
			<div className={styles.fieldLabel}>
				<label htmlFor={id}>{label}</label>
				<HelpTip label={label} description={utmDescriptions[key]} />
			</div>
			<input
				id={id}
				type="text"
				value={form[key] ?? ""}
				maxLength={200}
				onChange={(event) => setForm({ ...form, [key]: event.currentTarget.value })}
			/>
		</div>
	);

	return (
		<Dialog open={open} onOpenChange={onOpenChange} title={tag ? "Edit UTM tag" : "New UTM tag"} autoOverflow>
			<form onSubmit={submit} className={styles.form}>
				<label>
					Name
					<input
						type="text"
						value={form.displayName}
						onChange={(event) => setForm({ ...form, displayName: event.currentTarget.value })}
						required
						maxLength={100}
						autoFocus
					/>
				</label>
				<label>
					Destination URL
					<input
						type="text"
						inputMode="url"
						value={form.url}
						onChange={(event) => setForm({ ...form, url: event.currentTarget.value })}
						required
						maxLength={2048}
						placeholder="https://example.com/landing"
					/>
				</label>
				<p className={styles.groupHint}>
					<strong>UTM parameters.</strong> Fill in the ones your campaign uses; at least one is required. Reuse the same
					values across links so reports add them up.
				</p>
				<div className={styles.formFields}>
					{field("utmSource", "Source", `${fieldId}-source`)}
					{field("utmMedium", "Medium", `${fieldId}-medium`)}
					{field("utmCampaign", "Campaign", `${fieldId}-campaign`)}
					{field("utmContent", "Content", `${fieldId}-content`)}
					{field("utmTerm", "Term", `${fieldId}-term`)}
				</div>
				<div className={styles.preview}>
					<span className={styles.previewLabel}>Link preview</span>
					{preview ? (
						<div className={styles.previewValue}>
							<code>{preview}</code>
							<CopyButton value={preview} label="Link" />
						</div>
					) : (
						<p className={styles.previewHint}>Enter an absolute http or https URL to see the finished link.</p>
					)}
				</div>
				<div className="action-row">
					<Dialog.Close className="button-secondary">Cancel</Dialog.Close>
					<button type="submit" disabled={isPending}>
						{tag ? "Save changes" : "Create tag"}
					</button>
				</div>
			</form>
		</Dialog>
	);
};

export const DeleteUtmTagDialog = ({
	projectId,
	tag,
	open,
	onOpenChange,
}: {
	projectId: string;
	tag: UtmTag;
	open: boolean;
	onOpenChange: (open: boolean) => void;
}) => {
	const { mutate, isPending } = useMutation({
		mutationFn: () =>
			api["/api/dashboard/project/{project_id}/utm-tags/{tag_id}"].delete({
				params: { project_id: projectId, tag_id: tag.id },
			}),
		onSuccess: async () => {
			await invalidateUtmTags(projectId);
			createToast("UTM tag deleted", "success");
			onOpenChange(false);
		},
		onError: (error) => createToast(errorMessage(error, "Failed to delete UTM tag"), "error"),
	});

	return (
		<Dialog
			open={open}
			onOpenChange={onOpenChange}
			title={`Delete UTM tag: ${tag.displayName}`}
			description="This removes the saved link. Traffic already recorded is not affected."
		>
			<div className="action-row">
				<Dialog.Close className="button-secondary">Cancel</Dialog.Close>
				<button type="button" className="button-danger" onClick={() => mutate()} disabled={isPending}>
					Delete tag
				</button>
			</div>
		</Dialog>
	);
};
