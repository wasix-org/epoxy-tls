import { type Component, type ComponentChild } from "dreamland/core";
import Link from "./link";

const Git: Component<{ repo: string, children: ComponentChild }> = function(cx) {
	return <span><Link href={`https://github.com/${this.repo}`}>{cx.children}</Link></span>;
}

export default Git;