import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { QueryClientConfig } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { RenderResult } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";

export function renderWithQueryClient(
  ui: ReactElement,
  config?: QueryClientConfig,
): RenderResult & { queryClient: QueryClient } {
  const queryClient = new QueryClient(config);
  const result = render(ui, {
    wrapper: ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    ),
  });

  return { ...result, queryClient };
}
