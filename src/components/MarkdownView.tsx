import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api } from "../api/commands";

export default function MarkdownView({ content }: { content: string }) {
  return (
    <div className="markdown select-text cursor-text">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                e.preventDefault();
                if (href) api.openUrl(href).catch(console.error);
              }}
            >
              {children}
            </a>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
