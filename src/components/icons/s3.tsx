import { SVGProps } from "react";

export const S3Icon = (props: SVGProps<SVGSVGElement>) => (
	<svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" {...props}>
		<path
			d="M4 8.5L12 4l8 4.5v7L12 20l-8-4.5v-7z"
			fill="#F97316"
		/>
		<path d="M12 4v16" stroke="white" strokeWidth="1.4" />
		<path d="M4 8.5h16" stroke="white" strokeWidth="1.4" />
	</svg>
);
