"use server";

/**
 * Server action wrapping the inquiry submission. Called from the
 * `<InquiryModal>` client component. Returns the API's `InquiryAck`
 * verbatim so the client can render the right post-submit state
 * ("sent" vs "check your inbox").
 */

import { submitInquiry, type InquiryAck } from "@/lib/api";

export async function sendInquiry(
  artworkId: string,
  input: {
    name: string;
    email?: string;
    message: string;
    budget_range?: string;
  }
): Promise<InquiryAck> {
  return submitInquiry(artworkId, input);
}
