import styles from "./utm-tags.module.css";

import { useMemo, useState } from "react";

import { appPath, getDashboardProjectId } from "@/config";
import type { DimensionFilter, UtmTag } from "@/constants";
import { LoadingSpinner } from "@/components/ui/loading";
import { CopyButton } from "@/components/ui/snippet";
import { useMe, useProject, useUtmTagSummary, useUtmTags } from "@/hooks/api";
import { useRange } from "@/hooks/persist";
import { formatMetricVal, getUsername } from "@/utils";
import { ProjectHeader } from "../project-header";
import { ProjectNav } from "../project-nav";
import { SelectRange } from "../range";
import { serializeReportFilters } from "../report-filters";
import { buildUtmUrl } from "./build-url";
import { DeleteUtmTagDialog, UtmTagDialog } from "./dialog";

const intro =
	"Save the UTM-tagged links you use for a campaign. Each tag shows its pageviews and unique visitors for the selected range. Traffic reflects the tag's current UTM values.";

const adminEmpty = "No UTM tags yet. Save the campaign links you use to see how each one performs.";
const viewerEmpty = "No UTM tags have been added to this project.";

const tagValues = (tag: UtmTag) => ({
	source: tag.utmSource,
	medium: tag.utmMedium,
	campaign: tag.utmCampaign,
	content: tag.utmContent,
	term: tag.utmTerm,
});

const sourceMedium = (tag: UtmTag) => {
	if (!tag.utmSource && !tag.utmMedium) return "—";
	return `${tag.utmSource || "—"} / ${tag.utmMedium || "—"}`;
};

/** One `equal` filter per UTM value set on the tag, or `undefined` when there is none. */
const reportHref = (projectId: string, tag: UtmTag) => {
	const values: [DimensionFilter["dimension"], string | null | undefined][] = [
		["utm_source", tag.utmSource],
		["utm_medium", tag.utmMedium],
		["utm_campaign", tag.utmCampaign],
		["utm_content", tag.utmContent],
		["utm_term", tag.utmTerm],
	];
	const filters: DimensionFilter[] = values.flatMap(([dimension, value]) =>
		value ? [{ dimension, filterType: "equal" as const, value }] : [],
	);
	const serialized = serializeReportFilters(filters);
	return serialized ? appPath(`/p/${projectId}?filters=${encodeURIComponent(serialized)}`) : undefined;
};

export const UtmTags = () => {
	const [projectId] = useState(() => (typeof window === "undefined" ? undefined : getDashboardProjectId()));
	const { project, notFound } = useProject(projectId);
	// Anonymous visitors of a public project must not ask for a session they do not have.
	const { role } = useMe({ enabled: Boolean(getUsername()) });
	const isAdmin = role === "admin";
	const { range, setRange } = useRange();
	const { utmTags, isLoading, error } = useUtmTags(project?.id);
	const summary = useUtmTagSummary({ projectId: project?.id, range });
	const [editor, setEditor] = useState<{ tag: UtmTag | null } | null>(null);
	const [deleting, setDeleting] = useState<UtmTag | null>(null);

	const summaries = useMemo(() => new Map(summary.summaries.map((row) => [row.id, row])), [summary.summaries]);

	if (!projectId || notFound) {
		return <div className={styles.notFound}>Project not found</div>;
	}

	if (!project) {
		return (
			<div className={styles.loading} role="status" aria-label="Loading project">
				<LoadingSpinner />
			</div>
		);
	}

	const createButton = (
		<button type="button" onClick={() => setEditor({ tag: null })}>
			New UTM tag
		</button>
	);

	const empty = error
		? "Failed to load UTM tags."
		: isAdmin
			? adminEmpty
			: viewerEmpty;

	return (
		<div className={styles.screen}>
			<div className={styles.header}>
				<ProjectHeader project={project} />
				<SelectRange onSelect={setRange} range={range} projectId={project.id} />
			</div>
			<ProjectNav projectId={project.id} current="utm-tags" />
			<article className={styles.card}>
				<div className={styles.toolbar}>
					<p className={styles.intro}>{intro}</p>
					{isAdmin && createButton}
				</div>
				{isLoading ? (
					<div className={styles.loading}>
						<LoadingSpinner />
					</div>
				) : utmTags.length === 0 ? (
					<div className={styles.empty}>
						<p>{empty}</p>
						{isAdmin && !error && createButton}
					</div>
				) : (
					<div className={styles.tableScroll}>
						<table className={styles.table}>
							<thead>
								<tr>
									<th scope="col">Name</th>
									<th scope="col">Campaign</th>
									<th scope="col">Source / medium</th>
									<th scope="col" className={styles.number}>
										Views
									</th>
									<th scope="col" className={styles.number}>
										Visitors
									</th>
									<th scope="col">Link</th>
									<th scope="col">Actions</th>
								</tr>
							</thead>
							<tbody>
								{utmTags.map((tag) => {
									const link = buildUtmUrl(tag.url, tagValues(tag));
									const href = reportHref(project.id, tag);
									const row = summaries.get(tag.id);
									return (
										<tr key={tag.id}>
											<th scope="row" className={styles.name}>
												{tag.displayName}
											</th>
											<td>{tag.utmCampaign || "—"}</td>
											<td>{sourceMedium(tag)}</td>
											<td className={styles.number}>{formatMetricVal(row?.totalViews, "views")}</td>
											<td className={styles.number}>{formatMetricVal(row?.uniqueVisitors, "unique_visitors")}</td>
											<td>
												{link ? (
													<span className={styles.copyCell}>
														<CopyButton value={link} label="Link" />
													</span>
												) : (
													"—"
												)}
											</td>
											<td>
												<div className={styles.actions}>
													{href && <a href={href}>View traffic</a>}
													{isAdmin && (
														<>
															<button
																type="button"
																className="button-ghost"
																onClick={() => setEditor({ tag })}
															>
																Edit
															</button>
															<button
																type="button"
																className="button-ghost"
																onClick={() => setDeleting(tag)}
															>
																Delete
															</button>
														</>
													)}
												</div>
											</td>
										</tr>
									);
								})}
							</tbody>
						</table>
					</div>
				)}
			</article>
			{editor && (
				<UtmTagDialog
					key={editor.tag?.id ?? "new"}
					projectId={project.id}
					tag={editor.tag}
					open
					onOpenChange={(open) => !open && setEditor(null)}
				/>
			)}
			{deleting && (
				<DeleteUtmTagDialog
					projectId={project.id}
					tag={deleting}
					open
					onOpenChange={(open) => !open && setDeleting(null)}
				/>
			)}
		</div>
	);
};
